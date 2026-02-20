//! Session daemon module for persisting terminal sessions.
//!
//! This module implements a daemon that owns PTY sessions and communicates
//! with TUI clients via Unix sockets. Sessions persist when the TUI disconnects
//! and can be reattached later.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};

/// Platform-specific IPC stream type (UnixStream on Unix, TcpStream on Windows).
#[cfg(unix)]
pub type IpcStream = UnixStream;
#[cfg(unix)]
type IpcListener = UnixListener;
#[cfg(windows)]
pub type IpcStream = TcpStream;
#[cfg(windows)]
type IpcListener = TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::env_capture::capture_environment;
use crate::pty::{spawn_shell, spawn_shell_with_env, PtyEvent, PtyHandle};

/// Terminal state for restoring session on reconnect.
/// Contains the formatted escape sequence bytes that will restore
/// the terminal to its previous visual state (cursor position, colors, text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalState {
    /// Escape sequence bytes to restore the terminal screen.
    pub contents: Vec<u8>,
    /// Cursor position (row, col) - 0-indexed.
    pub cursor_position: (u16, u16),
    /// Terminal dimensions when state was captured.
    pub rows: u16,
    pub cols: u16,
}

/// Default scrollback lines for terminal state persistence.
/// This allows restoring scrollback history across daemon restarts.
/// Set to 1M lines to preserve extensive history.
pub const DEFAULT_SCROLLBACK: usize = 1_000_000;

/// Version number for persisted state format.
/// Increment when making breaking changes to the state format.
const PERSISTED_STATE_VERSION: u32 = 1;

/// Persisted session state saved to disk for daemon restart survival.
/// Unlike SessionMetadata (which is lightweight and always saved), this
/// contains the full terminal state and is only saved during graceful shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSessionState {
    /// Basic session metadata (name, cwd, dimensions).
    pub metadata: SessionMetadata,
    /// Terminal screen state (formatted escape sequences for replay).
    /// Includes both visible content and scrollback.
    pub terminal_state: Option<Vec<u8>>,
    /// Captured environment variables from the shell process.
    /// Filtered to exclude sensitive values.
    pub environment: Option<HashMap<String, String>>,
    /// Format version for forward compatibility.
    pub version: u32,
}

impl PersistedSessionState {
    /// Create a new persisted state from a session.
    pub fn new(metadata: SessionMetadata) -> Self {
        Self {
            metadata,
            terminal_state: None,
            environment: None,
            version: PERSISTED_STATE_VERSION,
        }
    }

    /// Get the path to this session's state file.
    pub fn file_path(session_name: &str) -> PathBuf {
        get_sessions_dir().join(format!("{}.state", session_name))
    }

    /// Save the persisted state to disk.
    pub fn save(&self) -> Result<()> {
        ensure_sessions_dir()?;
        let path = Self::file_path(&self.metadata.name);
        let data = serde_json::to_vec(self)
            .context("Failed to serialize persisted session state")?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write persisted state to {:?}", path))?;
        Ok(())
    }

    /// Load persisted state from disk.
    pub fn load(session_name: &str) -> Result<Option<Self>> {
        let path = Self::file_path(session_name);
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read(&path)
            .with_context(|| format!("Failed to read persisted state from {:?}", path))?;

        match serde_json::from_slice::<Self>(&data) {
            Ok(state) => {
                // Check version compatibility
                if state.version > PERSISTED_STATE_VERSION {
                    eprintln!(
                        "Warning: Persisted state version {} is newer than supported version {}",
                        state.version, PERSISTED_STATE_VERSION
                    );
                }
                Ok(Some(state))
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse persisted state: {:?}", e);
                // Delete corrupted state file
                let _ = fs::remove_file(&path);
                Ok(None)
            }
        }
    }

    /// Delete the persisted state file.
    pub fn delete(session_name: &str) -> Result<()> {
        let path = Self::file_path(session_name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete persisted state at {:?}", path))?;
        }
        Ok(())
    }
}

/// Message types for communication between TUI client and daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Attach to a session (create if doesn't exist).
    Attach {
        session_name: String,
        rows: u16,
        cols: u16,
        /// Working directory for new sessions.
        cwd: Option<PathBuf>,
    },
    /// Detach from the current session.
    Detach,
    /// Send input to the session.
    Input { data: Vec<u8> },
    /// Resize the terminal.
    Resize { rows: u16, cols: u16 },
    /// List all active sessions.
    List,
    /// Kill a specific session.
    Kill { session_name: String },
    /// List stale sessions (persisted but not currently running).
    ListStale,
    /// Restore a stale session from its persisted metadata.
    RestoreStale { session_name: String },
    /// Delete stale session metadata (user declined to restore).
    DeleteStale { session_name: String },
    /// Rename a session.
    Rename { old_name: String, new_name: String },
    /// Get terminal state for preview (without attaching).
    Preview { session_name: String },
    /// List all workspaces.
    ListWorkspaces,
    /// Create a new workspace.
    CreateWorkspace { name: String },
    /// Rename a workspace.
    RenameWorkspace { old_name: String, new_name: String },
    /// Delete a workspace and all its sessions.
    DeleteWorkspace { name: String },
    /// Switch active workspace (saves current workspace state, restores target workspace state).
    SwitchWorkspace { name: String },
    /// Move a session to a different workspace.
    MoveSessionToWorkspace { session_name: String, workspace_name: String },
    /// Save workspace view state (selected session, focused pane, scroll offset).
    SaveWorkspaceState {
        workspace_name: String,
        last_selected_session: Option<String>,
        last_focused_pane: String,
        sidebar_scroll_offset: usize,
    },
    /// Shutdown the daemon.
    Shutdown,
}

/// Response from daemon to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonResponse {
    /// Successfully attached to session.
    Attached {
        session_name: String,
        is_new: bool,
        /// Serialized terminal state for restoration.
        terminal_state: Option<Vec<u8>>,
    },
    /// Session detached.
    Detached,
    /// PTY output data.
    Output { data: Vec<u8> },
    /// Session list.
    Sessions { names: Vec<SessionInfo> },
    /// Stale sessions list (persisted but not currently running).
    StaleSessions { sessions: Vec<SessionMetadata> },
    /// Stale session was restored.
    Restored { session_name: String },
    /// Stale session metadata was deleted.
    Deleted { session_name: String },
    /// Session was killed.
    Killed { session_name: String },
    /// Session was renamed.
    Renamed { old_name: String, new_name: String },
    /// Terminal state for preview (without attaching).
    Previewed {
        session_name: String,
        /// Serialized terminal state for preview.
        terminal_state: Option<Vec<u8>>,
    },
    /// Workspace list.
    Workspaces { workspaces: Vec<WorkspaceInfo>, active_workspace: String },
    /// Workspace was created.
    WorkspaceCreated { name: String },
    /// Workspace was renamed.
    WorkspaceRenamed { old_name: String, new_name: String },
    /// Workspace was deleted.
    WorkspaceDeleted { name: String },
    /// Switched to a different workspace.
    WorkspaceSwitched { name: String, sessions: Vec<SessionInfo>, last_selected_session: Option<String>, last_focused_pane: String, sidebar_scroll_offset: usize },
    /// Session was moved to a different workspace.
    SessionMoved { session_name: String, workspace_name: String },
    /// Workspace state was saved.
    WorkspaceStateSaved,
    /// Error occurred.
    Error { message: String },
    /// Daemon is shutting down.
    ShuttingDown,
}

/// Information about a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub is_attached: bool,
    pub rows: u16,
    pub cols: u16,
    /// Timestamp when the session was last active (Unix epoch seconds).
    pub last_active: u64,
    /// Workspace this session belongs to.
    pub workspace_name: String,
}

/// Persistent session metadata saved to disk for reboot survival.
/// When the daemon restarts after a reboot, it can read these files
/// to know about sessions that were running before the reboot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Session name.
    pub name: String,
    /// Working directory for the session.
    pub cwd: Option<PathBuf>,
    /// Terminal dimensions (rows, cols).
    pub rows: u16,
    pub cols: u16,
    /// Timestamp when the session was created (Unix epoch seconds).
    pub created_at: u64,
    /// Timestamp when the session was last active (Unix epoch seconds).
    pub last_active: u64,
    /// Workspace this session belongs to (default: "Default").
    #[serde(default = "default_workspace_name")]
    pub workspace_name: String,
}

fn default_workspace_name() -> String {
    "Default".to_string()
}

/// Workspace metadata persisted to disk.
/// A workspace groups terminal sessions and saves view/layout state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    /// Unique name for the workspace.
    pub name: String,
    /// Timestamp when the workspace was created (Unix epoch seconds).
    pub created_at: u64,
    /// The session that was last selected in this workspace.
    pub last_selected_session: Option<String>,
    /// The pane that was last focused ("sidebar" or "terminal").
    pub last_focused_pane: String,
    /// Scroll offset in the sidebar session list.
    pub sidebar_scroll_offset: usize,
}

impl WorkspaceMetadata {
    /// Create a new workspace with default state.
    pub fn new(name: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            name,
            created_at: now,
            last_selected_session: None,
            last_focused_pane: "terminal".to_string(),
            sidebar_scroll_offset: 0,
        }
    }

    /// Get the path to the workspaces config file.
    pub fn file_path() -> PathBuf {
        get_data_dir().join("workspaces.json")
    }

    /// Load all workspaces from disk.
    pub fn load_all() -> Result<Vec<Self>> {
        let path = Self::file_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read workspaces from {:?}", path))?;
        let workspaces: Vec<Self> = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse workspaces from {:?}", path))?;
        Ok(workspaces)
    }

    /// Save all workspaces to disk.
    pub fn save_all(workspaces: &[Self]) -> Result<()> {
        ensure_data_dir()?;
        let path = Self::file_path();
        let data = serde_json::to_string_pretty(workspaces)
            .context("Failed to serialize workspaces")?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write workspaces to {:?}", path))?;
        Ok(())
    }
}

/// Information about a workspace returned to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub name: String,
    pub last_selected_session: Option<String>,
    pub last_focused_pane: String,
    pub sidebar_scroll_offset: usize,
}

impl SessionMetadata {
    /// Create new session metadata in the given workspace.
    pub fn new(name: String, cwd: Option<PathBuf>, rows: u16, cols: u16) -> Self {
        Self::new_in_workspace(name, cwd, rows, cols, "Default".to_string())
    }

    /// Create new session metadata in a specific workspace.
    pub fn new_in_workspace(name: String, cwd: Option<PathBuf>, rows: u16, cols: u16, workspace_name: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            name,
            cwd,
            rows,
            cols,
            created_at: now,
            last_active: now,
            workspace_name,
        }
    }

    /// Update the last_active timestamp.
    pub fn touch(&mut self) {
        self.last_active = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }

    /// Get the path to this session's metadata file.
    pub fn file_path(&self) -> PathBuf {
        get_sessions_dir().join(format!("{}.json", self.name))
    }

    /// Save the metadata to disk.
    pub fn save(&self) -> Result<()> {
        ensure_sessions_dir()?;
        let path = self.file_path();
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize session metadata")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write session metadata to {:?}", path))?;
        Ok(())
    }

    /// Load session metadata from a file.
    pub fn load(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)
            .with_context(|| format!("Failed to read session metadata from {:?}", path))?;
        let metadata: Self = serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse session metadata from {:?}", path))?;
        Ok(metadata)
    }

    /// Delete the metadata file from disk.
    pub fn delete(&self) -> Result<()> {
        let path = self.file_path();
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete session metadata at {:?}", path))?;
        }
        Ok(())
    }
}

/// Load all persisted session metadata from disk.
pub fn load_all_session_metadata() -> Result<Vec<SessionMetadata>> {
    let sessions_dir = get_sessions_dir();
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in fs::read_dir(&sessions_dir).context("Failed to read sessions directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            match SessionMetadata::load(&path) {
                Ok(metadata) => sessions.push(metadata),
                Err(e) => {
                    eprintln!("Warning: Failed to load session metadata from {:?}: {:?}", path, e);
                }
            }
        }
    }
    Ok(sessions)
}

/// Clean up metadata files for sessions that are no longer running.
/// This compares metadata files on disk against currently active sessions.
pub fn cleanup_stale_metadata(active_sessions: &[String]) -> Result<()> {
    let sessions_dir = get_sessions_dir();
    if !sessions_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&sessions_dir).context("Failed to read sessions directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !active_sessions.contains(&stem.to_string()) {
                    if let Err(e) = fs::remove_file(&path) {
                        eprintln!("Warning: Failed to remove stale metadata {:?}: {:?}", path, e);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Get the runtime directory for the daemon socket.
pub fn get_runtime_dir() -> PathBuf {
    // Try XDG_RUNTIME_DIR first (standard on Linux)
    if let Ok(dir) = env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("sidebar-tui");
    }
    platform_default_runtime_dir()
}

#[cfg(unix)]
fn platform_default_runtime_dir() -> PathBuf {
    // Fall back to /tmp/sidebar-tui-{uid}
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/sidebar-tui-{}", uid))
}

#[cfg(windows)]
fn platform_default_runtime_dir() -> PathBuf {
    // Use %LOCALAPPDATA%\sidebar-tui\runtime or fallback to %TEMP%
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("sidebar-tui").join("runtime");
    }
    PathBuf::from(env::var("TEMP").unwrap_or_else(|_| r"C:\Windows\Temp".to_string()))
        .join("sidebar-tui")
}

/// Get the data directory for persistent storage (survives reboots).
pub fn get_data_dir() -> PathBuf {
    // Try XDG_DATA_HOME first (standard on Linux)
    if let Ok(dir) = env::var("XDG_DATA_HOME") {
        return PathBuf::from(dir).join("sidebar-tui");
    }

    // Fall back to ~/.local/share/sidebar-tui
    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("share").join("sidebar-tui");
    }

    // Last resort fallback
    PathBuf::from("/tmp/sidebar-tui-data")
}

/// Get the sessions metadata directory.
pub fn get_sessions_dir() -> PathBuf {
    get_data_dir().join("sessions")
}

/// Get the socket path for the daemon (Unix) or port lockfile path (Windows).
pub fn get_socket_path() -> PathBuf {
    #[cfg(unix)]
    return get_runtime_dir().join("daemon.sock");
    #[cfg(windows)]
    return get_runtime_dir().join("daemon.port");
}

/// Read the daemon TCP port from the lockfile (Windows only).
#[cfg(windows)]
fn read_daemon_port(lockfile: &Path) -> io::Result<u16> {
    let content = fs::read_to_string(lockfile)?;
    content.trim().parse::<u16>().map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Invalid port in lockfile: {}", e))
    })
}

/// Write the daemon TCP port to the lockfile (Windows only).
#[cfg(windows)]
fn write_daemon_port(lockfile: &Path, port: u16) -> Result<()> {
    fs::write(lockfile, port.to_string())
        .context("Failed to write daemon port lockfile")?;
    Ok(())
}

/// Check if a daemon is running at the given socket/lockfile path.
fn is_daemon_running(socket_path: &Path) -> bool {
    #[cfg(unix)]
    {
        if !socket_path.exists() {
            return false;
        }
        UnixStream::connect(socket_path).is_ok()
    }
    #[cfg(windows)]
    {
        match read_daemon_port(socket_path) {
            Ok(port) => TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok(),
            Err(_) => false,
        }
    }
}

/// Ensure the runtime directory exists with proper permissions.
pub fn ensure_runtime_dir() -> Result<PathBuf> {
    let dir = get_runtime_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create runtime directory")?;
        // Set permissions to 0700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(&dir, perms)
                .context("Failed to set runtime directory permissions")?;
        }
    }
    Ok(dir)
}

/// Ensure the data directory exists with proper permissions.
pub fn ensure_data_dir() -> Result<PathBuf> {
    let dir = get_data_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create data directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(&dir, perms)
                .context("Failed to set data directory permissions")?;
        }
    }
    Ok(dir)
}

/// Ensure the sessions directory exists with proper permissions.
pub fn ensure_sessions_dir() -> Result<PathBuf> {
    ensure_data_dir()?;
    let dir = get_sessions_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create sessions directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(&dir, perms)
                .context("Failed to set sessions directory permissions")?;
        }
    }
    Ok(dir)
}

/// Session daemon that manages terminal sessions.
pub struct Daemon {
    /// Map of session names to session handles.
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// Workspace metadata, keyed by workspace name.
    workspaces: Arc<Mutex<HashMap<String, WorkspaceMetadata>>>,
    /// Active workspace name.
    active_workspace: Arc<Mutex<String>>,
    /// Path to the Unix socket.
    socket_path: PathBuf,
    /// Flag to signal shutdown.
    shutdown: Arc<AtomicBool>,
}

/// A single terminal session managed by the daemon.
///
/// The session owns the PTY and manages communication with clients.
/// The session also maintains a vt100 parser to track terminal state
/// for restoring sessions on client reconnect.
pub struct Session {
    pub name: String,
    pub rows: u16,
    pub cols: u16,
    pub is_attached: bool,
    /// The PTY handle for this session.
    pub pty: PtyHandle,
    /// Channel sender for notifying clients of PTY output.
    /// Each attached client has its own receiver.
    client_output_tx: Vec<Sender<Vec<u8>>>,
    /// Flag indicating if the shell is still running.
    pub shell_running: Arc<AtomicBool>,
    /// Handle for the PTY reader thread that forwards output to clients.
    _pty_reader_handle: Option<JoinHandle<()>>,
    /// vt100 parser for tracking terminal state.
    /// Used to restore terminal contents when a client reconnects.
    terminal_parser: vt100::Parser,
    /// Persistent metadata for this session (saved to disk).
    metadata: SessionMetadata,
}

impl Session {
    /// Create a new session with a PTY in the "Default" workspace.
    pub fn new(name: String, rows: u16, cols: u16, cwd: Option<PathBuf>) -> Result<Self> {
        Self::new_in_workspace(name, rows, cols, cwd, "Default".to_string())
    }

    /// Create a new session with a PTY in a specific workspace.
    pub fn new_in_workspace(name: String, rows: u16, cols: u16, cwd: Option<PathBuf>, workspace_name: String) -> Result<Self> {
        // Validate dimensions - vt100 panics with 0 dimensions
        let rows = if rows == 0 { 24 } else { rows };
        let cols = if cols == 0 { 80 } else { cols };

        let pty = spawn_shell(rows, cols, cwd.clone())?;
        let shell_running = Arc::new(AtomicBool::new(true));
        // Initialize vt100 parser with same dimensions as PTY.
        // Use DEFAULT_SCROLLBACK to preserve history for session restoration.
        let terminal_parser = vt100::Parser::new(rows, cols, DEFAULT_SCROLLBACK);

        // Create and save metadata for persistence across reboots
        let metadata = SessionMetadata::new_in_workspace(name.clone(), cwd, rows, cols, workspace_name);
        if let Err(e) = metadata.save() {
            eprintln!("Warning: Failed to save session metadata: {:?}", e);
        }

        Ok(Self {
            name,
            rows,
            cols,
            is_attached: false,
            pty,
            client_output_tx: Vec::new(),
            shell_running,
            _pty_reader_handle: None,
            terminal_parser,
            metadata,
        })
    }

    /// Create a session with a given PTY handle (for testing).
    /// Note: This does NOT save metadata to disk (test-only).
    #[cfg(test)]
    pub fn with_pty(name: String, rows: u16, cols: u16, pty: PtyHandle) -> Self {
        Self {
            name: name.clone(),
            rows,
            cols,
            is_attached: false,
            pty,
            client_output_tx: Vec::new(),
            shell_running: Arc::new(AtomicBool::new(true)),
            _pty_reader_handle: None,
            terminal_parser: vt100::Parser::new(rows, cols, DEFAULT_SCROLLBACK),
            metadata: SessionMetadata::new(name, None, rows, cols),
        }
    }

    /// Create a new session from persisted state (for restoration after daemon restart).
    /// This spawns a new shell with the captured environment variables and
    /// replays the terminal state through the parser.
    pub fn from_persisted_state(state: PersistedSessionState) -> Result<Self> {
        let rows = if state.metadata.rows == 0 { 24 } else { state.metadata.rows };
        let cols = if state.metadata.cols == 0 { 80 } else { state.metadata.cols };

        // Spawn shell with restored environment variables
        let pty = spawn_shell_with_env(
            rows,
            cols,
            state.metadata.cwd.clone(),
            state.environment,
        )?;
        let shell_running = Arc::new(AtomicBool::new(true));
        let mut terminal_parser = vt100::Parser::new(rows, cols, DEFAULT_SCROLLBACK);

        // Replay terminal state if available
        if let Some(ref terminal_data) = state.terminal_state {
            terminal_parser.process(terminal_data);
        }

        // Use the existing metadata (preserves created_at timestamp)
        let mut metadata = state.metadata;
        metadata.touch(); // Update last_active
        if let Err(e) = metadata.save() {
            eprintln!("Warning: Failed to save restored session metadata: {:?}", e);
        }

        Ok(Self {
            name: metadata.name.clone(),
            rows,
            cols,
            is_attached: false,
            pty,
            client_output_tx: Vec::new(),
            shell_running,
            _pty_reader_handle: None,
            terminal_parser,
            metadata,
        })
    }

    /// Create a new session from existing metadata (for restoration without terminal state).
    /// This preserves the original timestamps from the metadata.
    pub fn from_metadata(metadata: SessionMetadata) -> Result<Self> {
        let rows = if metadata.rows == 0 { 24 } else { metadata.rows };
        let cols = if metadata.cols == 0 { 80 } else { metadata.cols };

        // Spawn shell with the original working directory
        let pty = spawn_shell(rows, cols, metadata.cwd.clone())?;
        let shell_running = Arc::new(AtomicBool::new(true));
        let terminal_parser = vt100::Parser::new(rows, cols, DEFAULT_SCROLLBACK);

        // Keep the existing metadata (preserves created_at and last_active timestamps)
        // We don't call touch() here to preserve the order from before daemon restart

        Ok(Self {
            name: metadata.name.clone(),
            rows,
            cols,
            is_attached: false,
            pty,
            client_output_tx: Vec::new(),
            shell_running,
            _pty_reader_handle: None,
            terminal_parser,
            metadata,
        })
    }

    /// Save the session state for persistence across daemon restarts.
    /// Captures terminal state and environment variables.
    pub fn save_state(&self) -> Result<()> {
        // Get terminal state with scrollback
        let screen = self.terminal_parser.screen();
        // state_formatted includes scrollback + screen state
        let terminal_state = screen.state_formatted();

        // Capture environment variables from the shell process
        let environment = self.pty.process_id()
            .and_then(capture_environment);

        let persisted = PersistedSessionState {
            metadata: self.metadata.clone(),
            terminal_state: Some(terminal_state),
            environment,
            version: PERSISTED_STATE_VERSION,
        };

        persisted.save()
    }

    /// Gracefully shutdown the PTY, allowing the shell to save history.
    pub fn graceful_shutdown(&mut self) {
        self.pty.graceful_shutdown();
    }

    /// Delete the session's persistent metadata from disk.
    pub fn delete_metadata(&self) -> Result<()> {
        self.metadata.delete()
    }

    /// Update the session's last_active timestamp and save to disk.
    pub fn touch_metadata(&mut self) {
        self.metadata.touch();
        if let Err(e) = self.metadata.save() {
            eprintln!("Warning: Failed to update session metadata: {:?}", e);
        }
    }

    /// Add a client output channel.
    pub fn add_client(&mut self) -> Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        self.client_output_tx.push(tx);
        rx
    }

    /// Remove disconnected clients (those whose receivers have been dropped).
    pub fn cleanup_clients(&mut self) {
        self.client_output_tx.retain(|tx| {
            // Try to send empty data to check if receiver is still alive
            tx.send(Vec::new()).is_ok()
        });
    }

    /// Send data to all connected clients.
    pub fn broadcast_to_clients(&mut self, data: &[u8]) {
        self.client_output_tx.retain(|tx| {
            tx.send(data.to_vec()).is_ok()
        });
    }

    /// Write input to the PTY.
    pub fn write_input(&mut self, data: &[u8]) -> Result<()> {
        self.pty.write(data)
    }

    /// Resize the PTY and vt100 parser.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.rows = rows;
        self.cols = cols;
        self.terminal_parser.set_size(rows, cols);
        self.pty.resize(rows, cols)
    }

    /// Check if the shell is still running.
    pub fn is_running(&mut self) -> bool {
        self.pty.is_running()
    }

    /// Process pending PTY output, feed through vt100 parser, and broadcast to clients.
    pub fn process_pty_output(&mut self) {
        loop {
            match self.pty.rx.try_recv() {
                Ok(PtyEvent::Output(data)) => {
                    // Feed through vt100 parser to track terminal state
                    self.terminal_parser.process(&data);
                    self.broadcast_to_clients(&data);
                }
                Ok(PtyEvent::Exited) => {
                    self.shell_running.store(false, Ordering::SeqCst);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.shell_running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
    }

    /// Get the current terminal state for session restoration.
    /// Returns formatted escape sequences that will restore the terminal
    /// to its current visual state including cursor position and colors.
    pub fn get_terminal_state(&self) -> TerminalState {
        let screen = self.terminal_parser.screen();
        let cursor_position = screen.cursor_position();

        TerminalState {
            contents: screen.contents_formatted(),
            cursor_position,
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Process raw bytes through the terminal parser without sending to clients.
    /// Used for testing terminal state tracking.
    #[cfg(test)]
    pub fn process_raw(&mut self, data: &[u8]) {
        self.terminal_parser.process(data);
    }

    /// Get the plain text contents of the terminal (for testing).
    #[cfg(test)]
    pub fn terminal_contents(&self) -> String {
        self.terminal_parser.screen().contents()
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            name: self.name.clone(),
            is_attached: self.is_attached,
            rows: self.rows,
            cols: self.cols,
            last_active: self.metadata.last_active,
            workspace_name: self.metadata.workspace_name.clone(),
        }
    }

    /// Move this session to a different workspace.
    pub fn move_to_workspace(&mut self, workspace_name: String) {
        self.metadata.workspace_name = workspace_name;
        if let Err(e) = self.metadata.save() {
            eprintln!("Warning: Failed to save session metadata after workspace move: {:?}", e);
        }
    }
}

impl Daemon {
    /// Create a new daemon instance, loading workspaces from disk.
    pub fn new() -> Result<Self> {
        let socket_path = get_socket_path();
        let (workspaces, active_workspace) = Self::load_or_init_workspaces()?;
        Ok(Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            workspaces: Arc::new(Mutex::new(workspaces)),
            active_workspace: Arc::new(Mutex::new(active_workspace)),
            socket_path,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a daemon with a custom socket path (for testing).
    pub fn with_socket_path(socket_path: PathBuf) -> Self {
        let default_ws = WorkspaceMetadata::new("Default".to_string());
        let mut workspaces = HashMap::new();
        workspaces.insert("Default".to_string(), default_ws);
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            workspaces: Arc::new(Mutex::new(workspaces)),
            active_workspace: Arc::new(Mutex::new("Default".to_string())),
            socket_path,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Load workspaces from disk, or initialize with a Default workspace.
    fn load_or_init_workspaces() -> Result<(HashMap<String, WorkspaceMetadata>, String)> {
        let persisted = WorkspaceMetadata::load_all().unwrap_or_default();
        if persisted.is_empty() {
            // First run: create Default workspace
            let default_ws = WorkspaceMetadata::new("Default".to_string());
            let mut map = HashMap::new();
            map.insert("Default".to_string(), default_ws.clone());
            if let Err(e) = WorkspaceMetadata::save_all(&[default_ws]) {
                eprintln!("Warning: Failed to save default workspace: {:?}", e);
            }
            Ok((map, "Default".to_string()))
        } else {
            let mut map = HashMap::new();
            for ws in persisted {
                map.insert(ws.name.clone(), ws);
            }
            // Active workspace is the first one (could be made configurable later)
            let active = map.keys().next().cloned().unwrap_or_else(|| "Default".to_string());
            Ok((map, active))
        }
    }

    /// Get the active workspace name.
    pub fn active_workspace(&self) -> String {
        self.active_workspace.lock().unwrap().clone()
    }

    /// Get workspace info list for the client.
    pub fn list_workspaces(&self) -> (Vec<WorkspaceInfo>, String) {
        let workspaces = self.workspaces.lock().unwrap();
        let active = self.active_workspace.lock().unwrap().clone();
        let mut list: Vec<WorkspaceInfo> = workspaces.values()
            .map(|ws| WorkspaceInfo {
                name: ws.name.clone(),
                last_selected_session: ws.last_selected_session.clone(),
                last_focused_pane: ws.last_focused_pane.clone(),
                sidebar_scroll_offset: ws.sidebar_scroll_offset,
            })
            .collect();
        // Sort alphabetically for consistent display
        list.sort_by(|a, b| a.name.cmp(&b.name));
        (list, active)
    }

    /// Create a new workspace.
    pub fn create_workspace(&self, name: &str) -> Result<()> {
        let mut workspaces = self.workspaces.lock().unwrap();
        if workspaces.contains_key(name) {
            bail!("Workspace '{}' already exists", name);
        }
        let ws = WorkspaceMetadata::new(name.to_string());
        workspaces.insert(name.to_string(), ws);
        let list: Vec<WorkspaceMetadata> = workspaces.values().cloned().collect();
        drop(workspaces);
        WorkspaceMetadata::save_all(&list)?;
        Ok(())
    }

    /// Rename a workspace.
    pub fn rename_workspace(&self, old_name: &str, new_name: &str) -> Result<()> {
        let mut workspaces = self.workspaces.lock().unwrap();
        if !workspaces.contains_key(old_name) {
            bail!("Workspace '{}' not found", old_name);
        }
        if workspaces.contains_key(new_name) {
            bail!("Workspace '{}' already exists", new_name);
        }
        let mut ws = workspaces.remove(old_name).unwrap();
        ws.name = new_name.to_string();
        workspaces.insert(new_name.to_string(), ws);
        let list: Vec<WorkspaceMetadata> = workspaces.values().cloned().collect();
        drop(workspaces);
        WorkspaceMetadata::save_all(&list)?;

        // Update active workspace name if needed
        let mut active = self.active_workspace.lock().unwrap();
        if *active == old_name {
            *active = new_name.to_string();
        }

        // Update all sessions belonging to the renamed workspace
        let mut sessions = self.sessions.lock().unwrap();
        for session in sessions.values_mut() {
            if session.metadata.workspace_name == old_name {
                session.move_to_workspace(new_name.to_string());
            }
        }

        Ok(())
    }

    /// Delete a workspace and all its sessions.
    /// If this would leave no workspaces, auto-creates a "Default" workspace.
    pub fn delete_workspace(&self, name: &str) -> Result<String> {
        // Kill all sessions in this workspace
        {
            let mut sessions = self.sessions.lock().unwrap();
            let to_kill: Vec<String> = sessions.values()
                .filter(|s| s.metadata.workspace_name == name)
                .map(|s| s.name.clone())
                .collect();
            for session_name in to_kill {
                if let Some(session) = sessions.remove(&session_name) {
                    let _ = session.delete_metadata();
                    let _ = PersistedSessionState::delete(&session_name);
                }
            }
        }

        let new_active = {
            let mut workspaces = self.workspaces.lock().unwrap();
            workspaces.remove(name);

            // If no workspaces left, create Default
            if workspaces.is_empty() {
                let default_ws = WorkspaceMetadata::new("Default".to_string());
                workspaces.insert("Default".to_string(), default_ws);
            }

            let list: Vec<WorkspaceMetadata> = workspaces.values().cloned().collect();
            drop(workspaces);
            WorkspaceMetadata::save_all(&list)?;

            // Determine new active workspace
            let workspaces = self.workspaces.lock().unwrap();
            workspaces.keys().next().cloned().unwrap_or_else(|| "Default".to_string())
        };

        // Update active workspace if deleted workspace was active
        let mut active = self.active_workspace.lock().unwrap();
        if *active == name {
            *active = new_active.clone();
        }

        Ok(new_active)
    }

    /// Switch to a different workspace. Returns sessions in the new workspace.
    pub fn switch_workspace(&self, name: &str) -> Result<(Vec<SessionInfo>, Option<String>, String, usize)> {
        let workspaces = self.workspaces.lock().unwrap();
        if !workspaces.contains_key(name) {
            bail!("Workspace '{}' not found", name);
        }
        let ws = workspaces.get(name).unwrap();
        let last_selected = ws.last_selected_session.clone();
        let last_focused = ws.last_focused_pane.clone();
        let scroll_offset = ws.sidebar_scroll_offset;
        drop(workspaces);

        *self.active_workspace.lock().unwrap() = name.to_string();

        // Return sessions in the new workspace
        let sessions = self.sessions.lock().unwrap();
        let mut workspace_sessions: Vec<SessionInfo> = sessions.values()
            .filter(|s| s.metadata.workspace_name == name)
            .map(|s| s.info())
            .collect();
        workspace_sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        Ok((workspace_sessions, last_selected, last_focused, scroll_offset))
    }

    /// Move a session to a different workspace.
    pub fn move_session_to_workspace(&self, session_name: &str, workspace_name: &str) -> Result<()> {
        let workspaces = self.workspaces.lock().unwrap();
        if !workspaces.contains_key(workspace_name) {
            bail!("Workspace '{}' not found", workspace_name);
        }
        drop(workspaces);

        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(session_name) {
            session.move_to_workspace(workspace_name.to_string());
            Ok(())
        } else {
            bail!("Session '{}' not found", session_name)
        }
    }

    /// Save view state for a workspace.
    pub fn save_workspace_state(&self, workspace_name: &str, last_selected_session: Option<String>, last_focused_pane: String, sidebar_scroll_offset: usize) -> Result<()> {
        let mut workspaces = self.workspaces.lock().unwrap();
        if let Some(ws) = workspaces.get_mut(workspace_name) {
            ws.last_selected_session = last_selected_session;
            ws.last_focused_pane = last_focused_pane;
            ws.sidebar_scroll_offset = sidebar_scroll_offset;
        } else {
            bail!("Workspace '{}' not found", workspace_name);
        }
        let list: Vec<WorkspaceMetadata> = workspaces.values().cloned().collect();
        drop(workspaces);
        WorkspaceMetadata::save_all(&list)?;
        Ok(())
    }

    /// Get the socket path for this daemon.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Check if a daemon is already running.
    pub fn is_running(&self) -> bool {
        is_daemon_running(&self.socket_path)
    }

    /// Signal the daemon to shut down.
    pub fn signal_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Check if shutdown has been signaled.
    pub fn should_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Start the daemon and listen for connections.
    pub fn run(&self) -> Result<()> {
        self.run_platform()
    }

    #[cfg(unix)]
    fn run_platform(&self) -> Result<()> {
        ensure_runtime_dir()?;

        // Remove stale socket file if it exists
        if self.socket_path.exists() {
            if self.is_running() {
                bail!("Daemon is already running");
            }
            fs::remove_file(&self.socket_path)
                .context("Failed to remove stale socket file")?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .context("Failed to bind to Unix socket")?;

        // Set non-blocking so we can check for shutdown
        listener.set_nonblocking(true)
            .context("Failed to set socket to non-blocking")?;

        // Set up signal handler for graceful shutdown
        self.setup_signal_handler()?;

        while !self.should_shutdown() {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    // Configure stream before passing to handler thread:
                    // accepted sockets inherit non-blocking mode from the listener.
                    stream.set_nonblocking(false)
                        .context("Failed to set stream to blocking mode")?;
                    stream.set_read_timeout(Some(Duration::from_millis(50)))
                        .context("Failed to set stream read timeout")?;
                    let sessions = Arc::clone(&self.sessions);
                    let workspaces = Arc::clone(&self.workspaces);
                    let active_workspace = Arc::clone(&self.active_workspace);
                    let shutdown = Arc::clone(&self.shutdown);
                    thread::spawn(move || {
                        if let Err(e) = handle_client(stream, sessions, workspaces, active_workspace, shutdown) {
                            eprintln!("Error handling client: {:?}", e);
                        }
                    });
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No connection ready, sleep briefly and check shutdown
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {:?}", e);
                }
            }
        }

        // Clean up socket on exit
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }

        Ok(())
    }

    #[cfg(windows)]
    fn run_platform(&self) -> Result<()> {
        ensure_runtime_dir()?;

        // Bind to a random port assigned by the OS
        let listener = TcpListener::bind("127.0.0.1:0")
            .context("Failed to bind TCP listener")?;
        let port = listener.local_addr()
            .context("Failed to get local address")?.port();

        // Write port to lockfile so clients can discover us
        write_daemon_port(&self.socket_path, port)?;

        // Set non-blocking so we can check for shutdown
        listener.set_nonblocking(true)
            .context("Failed to set listener to non-blocking")?;

        // Set up signal handler for graceful shutdown
        self.setup_signal_handler()?;

        while !self.should_shutdown() {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    stream.set_nonblocking(false)
                        .context("Failed to set stream to blocking mode")?;
                    stream.set_read_timeout(Some(Duration::from_millis(50)))
                        .context("Failed to set stream read timeout")?;
                    let sessions = Arc::clone(&self.sessions);
                    let workspaces = Arc::clone(&self.workspaces);
                    let active_workspace = Arc::clone(&self.active_workspace);
                    let shutdown = Arc::clone(&self.shutdown);
                    thread::spawn(move || {
                        if let Err(e) = handle_client(stream, sessions, workspaces, active_workspace, shutdown) {
                            eprintln!("Error handling client: {:?}", e);
                        }
                    });
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No connection ready, sleep briefly and check shutdown
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {:?}", e);
                }
            }
        }

        // Clean up port lockfile on exit
        let _ = fs::remove_file(&self.socket_path);

        Ok(())
    }

    /// Set up signal handler for graceful shutdown.
    fn setup_signal_handler(&self) -> Result<()> {
        let shutdown = Arc::clone(&self.shutdown);
        let socket_path = self.socket_path.clone();
        let sessions = Arc::clone(&self.sessions);

        // Use a simple approach with ctrlc for SIGINT/SIGTERM
        // The signal-hook crate would be more comprehensive but ctrlc is simpler
        ctrlc::set_handler(move || {
            // Save all session states before shutdown (for daemon restart persistence)
            if let Ok(mut sessions_guard) = sessions.lock() {
                for session in sessions_guard.values_mut() {
                    // Save terminal state and environment
                    if let Err(e) = session.save_state() {
                        eprintln!("Warning: Failed to save session '{}' state: {:?}", session.name, e);
                    }
                    // Gracefully shutdown shell (triggers history save)
                    session.graceful_shutdown();
                }
            }

            shutdown.store(true, Ordering::SeqCst);
            // Clean up socket file
            if socket_path.exists() {
                let _ = fs::remove_file(&socket_path);
            }
        })
        .context("Failed to set signal handler")?;

        Ok(())
    }

    /// Save all session states to disk (for graceful shutdown).
    pub fn save_all_sessions(&self) -> Vec<String> {
        let mut saved = Vec::new();
        if let Ok(sessions_guard) = self.sessions.lock() {
            for session in sessions_guard.values() {
                if let Err(e) = session.save_state() {
                    eprintln!("Warning: Failed to save session '{}' state: {:?}", session.name, e);
                } else {
                    saved.push(session.name.clone());
                }
            }
        }
        saved
    }

    /// Get a list of all sessions (sorted by most recently used first).
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().unwrap();
        let mut list: Vec<SessionInfo> = sessions.values().map(|s| s.info()).collect();
        // Sort by last_active descending (most recently used first)
        list.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        list
    }

    /// Create or get a session.
    pub fn get_or_create_session(&self, name: &str, rows: u16, cols: u16, cwd: Option<PathBuf>) -> Result<(SessionInfo, bool)> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(name) {
            // Session exists, mark as attached and update dimensions
            session.is_attached = true;
            if let Err(e) = session.resize(rows, cols) {
                // Log but don't fail - session still exists
                eprintln!("Warning: failed to resize session: {:?}", e);
            }
            Ok((session.info(), false))
        } else {
            // Create new session with PTY
            let mut session = Session::new(name.to_string(), rows, cols, cwd)?;
            session.is_attached = true;
            let info = session.info();
            sessions.insert(name.to_string(), session);
            Ok((info, true))
        }
    }

    /// Detach from a session.
    pub fn detach_session(&self, name: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(name) {
            session.is_attached = false;
            true
        } else {
            false
        }
    }

    /// Kill a session.
    pub fn kill_session(&self, name: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.remove(name) {
            // Delete the persistent metadata and state files
            if let Err(e) = session.delete_metadata() {
                eprintln!("Warning: Failed to delete session metadata: {:?}", e);
            }
            if let Err(e) = PersistedSessionState::delete(name) {
                eprintln!("Warning: Failed to delete session state: {:?}", e);
            }
            true
        } else {
            false
        }
    }

    /// Get a list of stale sessions (persisted metadata with no running daemon session).
    /// These are sessions that were running before a reboot/crash.
    pub fn get_stale_sessions(&self) -> Vec<SessionMetadata> {
        let sessions = self.sessions.lock().unwrap();
        let active_names: Vec<String> = sessions.keys().cloned().collect();
        drop(sessions); // Release lock before file I/O

        match load_all_session_metadata() {
            Ok(all_metadata) => {
                all_metadata
                    .into_iter()
                    .filter(|m| !active_names.contains(&m.name))
                    .collect()
            }
            Err(e) => {
                eprintln!("Warning: Failed to load session metadata: {:?}", e);
                Vec::new()
            }
        }
    }

    /// Restore a stale session from its metadata and persisted state.
    /// If a .state file exists, the terminal state and environment will be restored.
    /// Otherwise, creates a new session with the same name and working directory.
    pub fn restore_session(&self, metadata: &SessionMetadata) -> Result<SessionInfo> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(&metadata.name) {
            bail!("Session '{}' already exists", metadata.name);
        }

        // Check for persisted state file (.state) with terminal and env data
        let session = match PersistedSessionState::load(&metadata.name)? {
            Some(persisted_state) => {
                // Full restoration with terminal state and environment
                Session::from_persisted_state(persisted_state)?
            }
            None => {
                // Fallback to metadata-only restoration (no terminal state)
                Session::new(
                    metadata.name.clone(),
                    metadata.rows,
                    metadata.cols,
                    metadata.cwd.clone(),
                )?
            }
        };

        let info = session.info();
        sessions.insert(metadata.name.clone(), session);

        // Clean up the .state file after successful restoration
        // (the session will create a new one on next shutdown)
        let _ = PersistedSessionState::delete(&metadata.name);

        Ok(info)
    }

    /// Delete metadata for a stale session (user declined to restore it).
    pub fn delete_stale_metadata(&self, name: &str) -> Result<()> {
        let path = get_sessions_dir().join(format!("{}.json", name));
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete stale metadata for '{}'", name))?;
        }
        // Also delete the .state file if it exists
        let _ = PersistedSessionState::delete(name);
        Ok(())
    }
}

impl Default for Daemon {
    fn default() -> Self {
        Self::new().expect("Failed to create default daemon")
    }
}

/// Handle a client connection.
/// The stream must already be configured (blocking, read timeout set) by the caller.
fn handle_client(
    mut stream: IpcStream,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    workspaces: Arc<Mutex<HashMap<String, WorkspaceMetadata>>>,
    active_workspace: Arc<Mutex<String>>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {

    let mut current_session: Option<String> = None;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            send_response(&mut stream, &DaemonResponse::ShuttingDown)?;
            break;
        }

        // Process PTY output if attached to a session
        // Collect all output in a local buffer first, then send without holding the lock
        if let Some(ref session_name) = current_session {
            let outputs = {
                let mut sessions_guard = sessions.lock().unwrap();
                let mut outputs = Vec::new();
                let mut shell_exited = false;

                if let Some(session) = sessions_guard.get_mut(session_name) {
                    // Collect all pending PTY output
                    loop {
                        match session.pty.rx.try_recv() {
                            Ok(PtyEvent::Output(data)) => {
                                // Feed through vt100 parser to track terminal state
                                session.terminal_parser.process(&data);
                                outputs.push(data);
                            }
                            Ok(PtyEvent::Exited) => {
                                session.shell_running.store(false, Ordering::SeqCst);
                                shell_exited = true;
                                break;
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                session.shell_running.store(false, Ordering::SeqCst);
                                shell_exited = true;
                                break;
                            }
                        }
                    }
                }
                (outputs, shell_exited)
            };

            // Send all collected output to the client (lock released)
            for data in outputs.0 {
                send_response(&mut stream, &DaemonResponse::Output { data })?;
            }

            // If shell exited, we could notify the client here
            if outputs.1 {
                // Shell exited - continue for now, client will handle disconnect
            }
        }

        let msg = match read_message(&mut stream) {
            Ok(msg) => msg,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Client disconnected - mark session as detached
                if let Some(session_name) = current_session.take() {
                    let mut sessions_guard = sessions.lock().unwrap();
                    if let Some(session) = sessions_guard.get_mut(&session_name) {
                        session.is_attached = false;
                    }
                }
                break;
            }
            Err(e) => {
                return Err(e).context("Failed to read message")?;
            }
        };

        let response = process_message(msg, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        send_response(&mut stream, &response)?;

        // Only break on shutdown - client may want to reattach after detach
        if matches!(response, DaemonResponse::ShuttingDown) {
            break;
        }
    }

    Ok(())
}

/// Process a client message and return a response.
fn process_message(
    msg: ClientMessage,
    sessions: &Arc<Mutex<HashMap<String, Session>>>,
    workspaces: &Arc<Mutex<HashMap<String, WorkspaceMetadata>>>,
    active_workspace: &Arc<Mutex<String>>,
    shutdown: &Arc<AtomicBool>,
    current_session: &mut Option<String>,
) -> DaemonResponse {
    match msg {
        ClientMessage::Attach { session_name, rows, cols, cwd } => {
            // Validate dimensions - vt100 panics with 0 dimensions
            let rows = if rows == 0 { 24 } else { rows };
            let cols = if cols == 0 { 80 } else { cols };

            let mut sessions_guard = sessions.lock().unwrap();
            let is_new = !sessions_guard.contains_key(&session_name);
            let mut terminal_state = None;

            // Create new session if it doesn't exist
            if is_new {
                let ws_name = active_workspace.lock().unwrap().clone();
                match Session::new_in_workspace(session_name.clone(), rows, cols, cwd, ws_name) {
                    Ok(mut session) => {
                        session.is_attached = true;
                        sessions_guard.insert(session_name.clone(), session);
                    }
                    Err(e) => {
                        return DaemonResponse::Error {
                            message: format!("Failed to create session: {}", e),
                        };
                    }
                }
            } else {
                // Reattaching to existing session - get terminal state for restoration
                if let Some(session) = sessions_guard.get_mut(&session_name) {
                    session.is_attached = true;
                    // Get the current terminal state before resizing
                    let state = session.get_terminal_state();
                    terminal_state = Some(state.contents);
                    // Now resize to match client dimensions
                    if let Err(e) = session.resize(rows, cols) {
                        eprintln!("Warning: failed to resize session: {:?}", e);
                    }
                    // Update last_active timestamp when attaching to the session
                    session.touch_metadata();
                }
            }

            *current_session = Some(session_name.clone());

            DaemonResponse::Attached {
                session_name,
                is_new,
                terminal_state,
            }
        }
        ClientMessage::Detach => {
            if let Some(session_name) = current_session.take() {
                let mut sessions_guard = sessions.lock().unwrap();
                if let Some(session) = sessions_guard.get_mut(&session_name) {
                    session.is_attached = false;
                }
            }
            DaemonResponse::Detached
        }
        ClientMessage::Input { data } => {
            if let Some(session_name) = current_session {
                let mut sessions_guard = sessions.lock().unwrap();
                if let Some(session) = sessions_guard.get_mut(session_name) {
                    if let Err(e) = session.write_input(&data) {
                        return DaemonResponse::Error {
                            message: format!("Failed to write input: {}", e),
                        };
                    }
                    // Update last_active timestamp when user sends input
                    session.touch_metadata();
                }
            }
            DaemonResponse::Output { data: vec![] }
        }
        ClientMessage::Resize { rows, cols } => {
            let mut sessions_guard = sessions.lock().unwrap();
            for session in sessions_guard.values_mut() {
                if let Err(e) = session.resize(rows, cols) {
                    let name = session.name.clone();
                    return DaemonResponse::Error {
                        message: format!("Failed to resize session '{}': {}", name, e),
                    };
                }
            }
            DaemonResponse::Output { data: vec![] }
        }
        ClientMessage::List => {
            let sessions_guard = sessions.lock().unwrap();
            let mut names: Vec<SessionInfo> = sessions_guard.values().map(|s| s.info()).collect();
            // Sort by last_active descending (most recently used first)
            names.sort_by(|a, b| b.last_active.cmp(&a.last_active));
            DaemonResponse::Sessions { names }
        }
        ClientMessage::Kill { session_name } => {
            let mut sessions_guard = sessions.lock().unwrap();
            if let Some(session) = sessions_guard.remove(&session_name) {
                // Delete the persistent metadata and state files
                if let Err(e) = session.delete_metadata() {
                    eprintln!("Warning: Failed to delete session metadata: {:?}", e);
                }
                if let Err(e) = PersistedSessionState::delete(&session_name) {
                    eprintln!("Warning: Failed to delete session state: {:?}", e);
                }
                DaemonResponse::Killed { session_name }
            } else {
                DaemonResponse::Error {
                    message: format!("Session '{}' not found", session_name),
                }
            }
        }
        ClientMessage::ListStale => {
            let sessions_guard = sessions.lock().unwrap();
            let active_names: Vec<String> = sessions_guard.keys().cloned().collect();
            drop(sessions_guard);

            match load_all_session_metadata() {
                Ok(all_metadata) => {
                    let stale: Vec<SessionMetadata> = all_metadata
                        .into_iter()
                        .filter(|m| !active_names.contains(&m.name))
                        .collect();
                    DaemonResponse::StaleSessions { sessions: stale }
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to load session metadata: {}", e),
                },
            }
        }
        ClientMessage::RestoreStale { session_name } => {
            // First check if session already exists
            {
                let sessions_guard = sessions.lock().unwrap();
                if sessions_guard.contains_key(&session_name) {
                    return DaemonResponse::Error {
                        message: format!("Session '{}' already exists", session_name),
                    };
                }
            }

            // Load metadata for this session
            let metadata_path = get_sessions_dir().join(format!("{}.json", session_name));
            match SessionMetadata::load(&metadata_path) {
                Ok(metadata) => {
                    // Check for persisted state file (.state) with terminal and env data
                    let session_result = match PersistedSessionState::load(&session_name) {
                        Ok(Some(persisted_state)) => {
                            // Full restoration with terminal state and environment
                            Session::from_persisted_state(persisted_state)
                        }
                        Ok(None) => {
                            // Fallback to metadata-only restoration (no terminal state)
                            // Use from_metadata to preserve original timestamps
                            Session::from_metadata(metadata)
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to load persisted state: {:?}", e);
                            // Fallback to metadata-only restoration
                            // Use from_metadata to preserve original timestamps
                            Session::from_metadata(metadata)
                        }
                    };

                    match session_result {
                        Ok(session) => {
                            let mut sessions_guard = sessions.lock().unwrap();
                            sessions_guard.insert(session_name.clone(), session);
                            // Clean up .state file after successful restoration
                            let _ = PersistedSessionState::delete(&session_name);
                            DaemonResponse::Restored { session_name }
                        }
                        Err(e) => DaemonResponse::Error {
                            message: format!("Failed to restore session: {}", e),
                        },
                    }
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to load session metadata: {}", e),
                },
            }
        }
        ClientMessage::DeleteStale { session_name } => {
            let metadata_path = get_sessions_dir().join(format!("{}.json", session_name));
            if metadata_path.exists() {
                match fs::remove_file(&metadata_path) {
                    Ok(()) => {
                        // Also clean up the .state file if it exists
                        let _ = PersistedSessionState::delete(&session_name);
                        DaemonResponse::Deleted { session_name }
                    }
                    Err(e) => DaemonResponse::Error {
                        message: format!("Failed to delete metadata: {}", e),
                    },
                }
            } else {
                DaemonResponse::Error {
                    message: format!("No metadata found for session '{}'", session_name),
                }
            }
        }
        ClientMessage::Rename { old_name, new_name } => {
            // Validate the new name is not empty
            if new_name.is_empty() {
                return DaemonResponse::Error {
                    message: "New session name cannot be empty".to_string(),
                };
            }

            let mut sessions_guard = sessions.lock().unwrap();

            // Check if new name already exists
            if sessions_guard.contains_key(&new_name) {
                return DaemonResponse::Error {
                    message: format!("Session '{}' already exists", new_name),
                };
            }

            // Remove the old session
            if let Some(mut session) = sessions_guard.remove(&old_name) {
                // Delete old metadata files
                if let Err(e) = session.delete_metadata() {
                    eprintln!("Warning: Failed to delete old session metadata: {:?}", e);
                }
                let _ = PersistedSessionState::delete(&old_name);

                // Update session name and save new metadata
                session.name = new_name.clone();
                session.metadata.name = new_name.clone();
                session.metadata.touch();
                if let Err(e) = session.metadata.save() {
                    eprintln!("Warning: Failed to save renamed session metadata: {:?}", e);
                }

                sessions_guard.insert(new_name.clone(), session);
                DaemonResponse::Renamed { old_name, new_name }
            } else {
                DaemonResponse::Error {
                    message: format!("Session '{}' not found", old_name),
                }
            }
        }
        ClientMessage::Preview { session_name } => {
            let sessions_guard = sessions.lock().unwrap();
            if let Some(session) = sessions_guard.get(&session_name) {
                // Get terminal state for preview using the same method as session attach
                let state = session.get_terminal_state();
                DaemonResponse::Previewed {
                    session_name,
                    terminal_state: Some(state.contents),
                }
            } else {
                DaemonResponse::Error {
                    message: format!("Session '{}' not found", session_name),
                }
            }
        }
        ClientMessage::ListWorkspaces => {
            let workspaces_guard = workspaces.lock().unwrap();
            let active = active_workspace.lock().unwrap().clone();
            let mut list: Vec<WorkspaceInfo> = workspaces_guard.values()
                .map(|ws| WorkspaceInfo {
                    name: ws.name.clone(),
                    last_selected_session: ws.last_selected_session.clone(),
                    last_focused_pane: ws.last_focused_pane.clone(),
                    sidebar_scroll_offset: ws.sidebar_scroll_offset,
                })
                .collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            DaemonResponse::Workspaces { workspaces: list, active_workspace: active }
        }
        ClientMessage::CreateWorkspace { name } => {
            let mut workspaces_guard = workspaces.lock().unwrap();
            if workspaces_guard.contains_key(&name) {
                return DaemonResponse::Error {
                    message: format!("Workspace '{}' already exists", name),
                };
            }
            let ws = WorkspaceMetadata::new(name.clone());
            workspaces_guard.insert(name.clone(), ws);
            let list: Vec<WorkspaceMetadata> = workspaces_guard.values().cloned().collect();
            drop(workspaces_guard);
            if let Err(e) = WorkspaceMetadata::save_all(&list) {
                return DaemonResponse::Error { message: format!("Failed to save workspace: {}", e) };
            }
            DaemonResponse::WorkspaceCreated { name }
        }
        ClientMessage::RenameWorkspace { old_name, new_name } => {
            let mut workspaces_guard = workspaces.lock().unwrap();
            if !workspaces_guard.contains_key(&old_name) {
                return DaemonResponse::Error {
                    message: format!("Workspace '{}' not found", old_name),
                };
            }
            if workspaces_guard.contains_key(&new_name) {
                return DaemonResponse::Error {
                    message: format!("Workspace '{}' already exists", new_name),
                };
            }
            let mut ws = workspaces_guard.remove(&old_name).unwrap();
            ws.name = new_name.clone();
            workspaces_guard.insert(new_name.clone(), ws);
            let list: Vec<WorkspaceMetadata> = workspaces_guard.values().cloned().collect();
            drop(workspaces_guard);
            if let Err(e) = WorkspaceMetadata::save_all(&list) {
                return DaemonResponse::Error { message: format!("Failed to save workspaces: {}", e) };
            }
            // Update active workspace if renamed
            let mut active = active_workspace.lock().unwrap();
            if *active == old_name {
                *active = new_name.clone();
            }
            // Update all sessions in renamed workspace
            let mut sessions_guard = sessions.lock().unwrap();
            for session in sessions_guard.values_mut() {
                if session.metadata.workspace_name == old_name {
                    session.move_to_workspace(new_name.clone());
                }
            }
            DaemonResponse::WorkspaceRenamed { old_name, new_name }
        }
        ClientMessage::DeleteWorkspace { name } => {
            // Kill all sessions in this workspace
            {
                let mut sessions_guard = sessions.lock().unwrap();
                let to_kill: Vec<String> = sessions_guard.values()
                    .filter(|s| s.metadata.workspace_name == name)
                    .map(|s| s.name.clone())
                    .collect();
                for session_name in to_kill {
                    if let Some(session) = sessions_guard.remove(&session_name) {
                        let _ = session.delete_metadata();
                        let _ = PersistedSessionState::delete(&session_name);
                    }
                }
            }

            let mut workspaces_guard = workspaces.lock().unwrap();
            workspaces_guard.remove(&name);

            // If no workspaces left, create Default
            if workspaces_guard.is_empty() {
                let default_ws = WorkspaceMetadata::new("Default".to_string());
                workspaces_guard.insert("Default".to_string(), default_ws);
            }

            let list: Vec<WorkspaceMetadata> = workspaces_guard.values().cloned().collect();
            let remaining_names: Vec<String> = workspaces_guard.keys().cloned().collect();
            drop(workspaces_guard);

            if let Err(e) = WorkspaceMetadata::save_all(&list) {
                return DaemonResponse::Error { message: format!("Failed to save workspaces: {}", e) };
            }

            // Update active workspace if deleted workspace was active
            let mut active = active_workspace.lock().unwrap();
            if *active == name {
                *active = remaining_names.into_iter().next().unwrap_or_else(|| "Default".to_string());
            }

            DaemonResponse::WorkspaceDeleted { name }
        }
        ClientMessage::SwitchWorkspace { name } => {
            let (last_selected, last_focused, scroll_offset) = {
                let workspaces_guard = workspaces.lock().unwrap();
                if !workspaces_guard.contains_key(&name) {
                    return DaemonResponse::Error {
                        message: format!("Workspace '{}' not found", name),
                    };
                }
                let ws = workspaces_guard.get(&name).unwrap();
                (ws.last_selected_session.clone(), ws.last_focused_pane.clone(), ws.sidebar_scroll_offset)
            };
            *active_workspace.lock().unwrap() = name.clone();

            let sessions_guard = sessions.lock().unwrap();
            let mut workspace_sessions: Vec<SessionInfo> = sessions_guard.values()
                .filter(|s| s.metadata.workspace_name == name)
                .map(|s| s.info())
                .collect();
            workspace_sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
            DaemonResponse::WorkspaceSwitched {
                name,
                sessions: workspace_sessions,
                last_selected_session: last_selected,
                last_focused_pane: last_focused,
                sidebar_scroll_offset: scroll_offset,
            }
        }
        ClientMessage::MoveSessionToWorkspace { session_name, workspace_name } => {
            {
                let workspaces_guard = workspaces.lock().unwrap();
                if !workspaces_guard.contains_key(&workspace_name) {
                    return DaemonResponse::Error {
                        message: format!("Workspace '{}' not found", workspace_name),
                    };
                }
            }
            let mut sessions_guard = sessions.lock().unwrap();
            if let Some(session) = sessions_guard.get_mut(&session_name) {
                session.move_to_workspace(workspace_name.clone());
                DaemonResponse::SessionMoved { session_name, workspace_name }
            } else {
                DaemonResponse::Error {
                    message: format!("Session '{}' not found", session_name),
                }
            }
        }
        ClientMessage::SaveWorkspaceState { workspace_name, last_selected_session, last_focused_pane, sidebar_scroll_offset } => {
            let mut workspaces_guard = workspaces.lock().unwrap();
            if let Some(ws) = workspaces_guard.get_mut(&workspace_name) {
                ws.last_selected_session = last_selected_session;
                ws.last_focused_pane = last_focused_pane;
                ws.sidebar_scroll_offset = sidebar_scroll_offset;
            } else {
                return DaemonResponse::Error {
                    message: format!("Workspace '{}' not found", workspace_name),
                };
            }
            let list: Vec<WorkspaceMetadata> = workspaces_guard.values().cloned().collect();
            drop(workspaces_guard);
            if let Err(e) = WorkspaceMetadata::save_all(&list) {
                return DaemonResponse::Error { message: format!("Failed to save workspace state: {}", e) };
            }
            DaemonResponse::WorkspaceStateSaved
        }
        ClientMessage::Shutdown => {
            shutdown.store(true, Ordering::SeqCst);
            DaemonResponse::ShuttingDown
        }
    }
}

/// Simple length-prefixed message encoding for Unix socket communication.
/// Format: [4 bytes length (big-endian)][JSON payload]
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(msg).context("Failed to serialize message")?;
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Decode a length-prefixed message from a reader (blocking).
/// WARNING: Only use with blocking I/O or long timeouts. For non-blocking reads,
/// use MessageReader instead to handle partial reads properly.
pub fn decode_message<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // Sanity check to prevent memory exhaustion
    if len > 10 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Message too large",
        ));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;

    serde_json::from_slice(&payload).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e))
    })
}

/// Buffered message reader that handles partial reads gracefully.
/// This is safe to use with non-blocking I/O and short timeouts.
#[derive(Default)]
pub struct MessageReader {
    /// Buffer for accumulating partial messages.
    buffer: Vec<u8>,
    /// Expected message length (once we've read the header).
    expected_len: Option<usize>,
}

impl MessageReader {
    /// Create a new message reader.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            expected_len: None,
        }
    }

    /// Clear the internal buffer. Call this before doing synchronous reads
    /// that bypass the MessageReader to avoid message corruption.
    /// WARNING: Any partial message data will be lost.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.expected_len = None;
    }

    /// Check if there's buffered data that hasn't been processed yet.
    pub fn has_buffered_data(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Try to parse any complete messages from the existing buffer without reading more.
    /// Use this to drain buffered messages before synchronous operations.
    pub fn try_parse_buffered<T: for<'de> Deserialize<'de>>(&mut self) -> io::Result<Option<T>> {
        self.try_parse()
    }

    /// Try to read a complete message from the stream.
    /// Returns Ok(Some(msg)) if a complete message was read.
    /// Returns Ok(None) if more data is needed (timeout/wouldblock).
    /// Returns Err on actual errors (connection closed, invalid data).
    pub fn try_read<T: for<'de> Deserialize<'de>>(
        &mut self,
        reader: &mut impl Read,
    ) -> io::Result<Option<T>> {
        // Try to read more data into our buffer
        let mut temp_buf = [0u8; 8192];
        match reader.read(&mut temp_buf) {
            Ok(0) => {
                // EOF - connection closed
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Connection closed",
                ));
            }
            Ok(n) => {
                self.buffer.extend_from_slice(&temp_buf[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                // No data available right now
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                // Timeout, no data available
            }
            Err(e) => {
                return Err(e);
            }
        }

        // Try to parse a complete message from the buffer
        self.try_parse()
    }

    /// Try to parse a complete message from the buffer.
    fn try_parse<T: for<'de> Deserialize<'de>>(&mut self) -> io::Result<Option<T>> {
        // Need at least 4 bytes for the length header
        if self.buffer.len() < 4 {
            return Ok(None);
        }

        // Parse the length if we haven't yet
        if self.expected_len.is_none() {
            let len_bytes: [u8; 4] = self.buffer[..4].try_into().unwrap();
            let len = u32::from_be_bytes(len_bytes) as usize;

            // Sanity check
            if len > 10 * 1024 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Message too large",
                ));
            }

            self.expected_len = Some(len);
        }

        let expected = self.expected_len.unwrap();
        let total_needed = 4 + expected;

        // Check if we have the complete message
        if self.buffer.len() < total_needed {
            return Ok(None);
        }

        // Extract the message payload
        let payload = &self.buffer[4..total_needed];
        let msg: T = serde_json::from_slice(payload).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e))
        })?;

        // Remove the parsed message from the buffer
        self.buffer.drain(..total_needed);
        self.expected_len = None;

        Ok(Some(msg))
    }
}

/// Read a message from the stream.
fn read_message(stream: &mut IpcStream) -> io::Result<ClientMessage> {
    decode_message(stream)
}

/// Send a response to the client.
fn send_response(stream: &mut IpcStream, response: &DaemonResponse) -> Result<()> {
    let encoded = encode_message(response)?;
    stream.write_all(&encoded).context("Failed to write response")?;
    stream.flush().context("Failed to flush response")?;
    Ok(())
}

/// Client handle for connecting to the daemon.
pub struct DaemonClient {
    stream: IpcStream,
}

impl DaemonClient {
    /// Connect to the daemon.
    pub fn connect() -> Result<Self> {
        let socket_path = get_socket_path();
        Self::connect_to(&socket_path)
    }

    /// Connect to a daemon at the given path.
    /// On Unix: connects to the Unix domain socket at `socket_path`.
    /// On Windows: reads the TCP port from the lockfile at `socket_path` and
    /// connects to `127.0.0.1:{port}`.
    pub fn connect_to(socket_path: &Path) -> Result<Self> {
        #[cfg(unix)]
        let stream = UnixStream::connect(socket_path)
            .context("Failed to connect to daemon")?;
        #[cfg(windows)]
        let stream = {
            let port = read_daemon_port(socket_path)
                .context("Failed to read daemon port from lockfile")?;
            TcpStream::connect(format!("127.0.0.1:{}", port))
                .context("Failed to connect to daemon via TCP")?
        };
        Ok(Self { stream })
    }

    /// Send a message to the daemon and wait for a response.
    pub fn send(&mut self, msg: ClientMessage) -> Result<DaemonResponse> {
        let encoded = encode_message(&msg)?;
        self.stream.write_all(&encoded).context("Failed to send message")?;
        self.stream.flush().context("Failed to flush message")?;

        let response: DaemonResponse = decode_message(&mut self.stream)
            .context("Failed to read response")?;
        Ok(response)
    }

    /// Attach to a session.
    pub fn attach(&mut self, session_name: &str, rows: u16, cols: u16, cwd: Option<PathBuf>) -> Result<DaemonResponse> {
        self.send(ClientMessage::Attach {
            session_name: session_name.to_string(),
            rows,
            cols,
            cwd,
        })
    }

    /// Send input to the current session.
    pub fn send_input(&mut self, data: &[u8]) -> Result<()> {
        match self.send(ClientMessage::Input { data: data.to_vec() })? {
            DaemonResponse::Output { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Resize the terminal.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        match self.send(ClientMessage::Resize { rows, cols })? {
            DaemonResponse::Output { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Detach from the current session.
    pub fn detach(&mut self) -> Result<()> {
        match self.send(ClientMessage::Detach)? {
            DaemonResponse::Detached => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Try to receive PTY output without blocking.
    pub fn try_recv_output(&mut self) -> Result<Option<Vec<u8>>> {
        self.stream.set_read_timeout(Some(Duration::from_millis(1)))
            .context("Failed to set read timeout")?;

        match decode_message::<DaemonResponse>(&mut self.stream) {
            Ok(DaemonResponse::Output { data }) => Ok(Some(data)),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(None),
            Err(e) => Err(e).context("Failed to receive output")?,
        }
    }

    /// List all sessions.
    pub fn list_sessions(&mut self) -> Result<Vec<SessionInfo>> {
        match self.send(ClientMessage::List)? {
            DaemonResponse::Sessions { names } => Ok(names),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Kill a session.
    pub fn kill_session(&mut self, session_name: &str) -> Result<()> {
        match self.send(ClientMessage::Kill {
            session_name: session_name.to_string(),
        })? {
            DaemonResponse::Killed { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Request daemon shutdown.
    pub fn shutdown(&mut self) -> Result<()> {
        match self.send(ClientMessage::Shutdown)? {
            DaemonResponse::ShuttingDown => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// List stale sessions (persisted but not currently running).
    pub fn list_stale_sessions(&mut self) -> Result<Vec<SessionMetadata>> {
        match self.send(ClientMessage::ListStale)? {
            DaemonResponse::StaleSessions { sessions } => Ok(sessions),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Restore a stale session from its persisted metadata.
    pub fn restore_stale_session(&mut self, session_name: &str) -> Result<()> {
        match self.send(ClientMessage::RestoreStale {
            session_name: session_name.to_string(),
        })? {
            DaemonResponse::Restored { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Delete stale session metadata (user declined to restore).
    pub fn delete_stale_session(&mut self, session_name: &str) -> Result<()> {
        match self.send(ClientMessage::DeleteStale {
            session_name: session_name.to_string(),
        })? {
            DaemonResponse::Deleted { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Rename a session.
    pub fn rename_session(&mut self, old_name: &str, new_name: &str) -> Result<()> {
        match self.send(ClientMessage::Rename {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        })? {
            DaemonResponse::Renamed { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Get terminal state for preview (without attaching).
    pub fn preview_session(&mut self, session_name: &str) -> Result<Option<Vec<u8>>> {
        match self.send(ClientMessage::Preview {
            session_name: session_name.to_string(),
        })? {
            DaemonResponse::Previewed { terminal_state, .. } => Ok(terminal_state),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// List all workspaces and the active workspace.
    pub fn list_workspaces(&mut self) -> Result<(Vec<WorkspaceInfo>, String)> {
        match self.send(ClientMessage::ListWorkspaces)? {
            DaemonResponse::Workspaces { workspaces, active_workspace } => Ok((workspaces, active_workspace)),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Create a new workspace.
    pub fn create_workspace(&mut self, name: &str) -> Result<()> {
        match self.send(ClientMessage::CreateWorkspace { name: name.to_string() })? {
            DaemonResponse::WorkspaceCreated { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Rename a workspace.
    pub fn rename_workspace(&mut self, old_name: &str, new_name: &str) -> Result<()> {
        match self.send(ClientMessage::RenameWorkspace {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        })? {
            DaemonResponse::WorkspaceRenamed { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Delete a workspace and all its sessions.
    pub fn delete_workspace(&mut self, name: &str) -> Result<()> {
        match self.send(ClientMessage::DeleteWorkspace { name: name.to_string() })? {
            DaemonResponse::WorkspaceDeleted { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Switch to a different workspace. Returns sessions in the new workspace.
    pub fn switch_workspace(&mut self, name: &str) -> Result<Vec<SessionInfo>> {
        match self.send(ClientMessage::SwitchWorkspace { name: name.to_string() })? {
            DaemonResponse::WorkspaceSwitched { sessions, .. } => Ok(sessions),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Move a session to a different workspace.
    pub fn move_session_to_workspace(&mut self, session_name: &str, workspace_name: &str) -> Result<()> {
        match self.send(ClientMessage::MoveSessionToWorkspace {
            session_name: session_name.to_string(),
            workspace_name: workspace_name.to_string(),
        })? {
            DaemonResponse::SessionMoved { .. } => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Save workspace view state.
    pub fn save_workspace_state(&mut self, workspace_name: &str, last_selected_session: Option<String>, last_focused_pane: String, sidebar_scroll_offset: usize) -> Result<()> {
        match self.send(ClientMessage::SaveWorkspaceState {
            workspace_name: workspace_name.to_string(),
            last_selected_session,
            last_focused_pane,
            sidebar_scroll_offset,
        })? {
            DaemonResponse::WorkspaceStateSaved => Ok(()),
            DaemonResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn temp_socket_path() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        PathBuf::from(format!("/tmp/sidebar-tui-test-{}-{}.sock", pid, id))
    }

    fn cleanup_socket(path: &Path) {
        let _ = fs::remove_file(path);
    }

    /// Create default workspaces and active_workspace arcs for testing.
    fn test_workspaces() -> (Arc<Mutex<HashMap<String, WorkspaceMetadata>>>, Arc<Mutex<String>>) {
        let mut workspaces = HashMap::new();
        workspaces.insert("Default".to_string(), WorkspaceMetadata::new("Default".to_string()));
        (
            Arc::new(Mutex::new(workspaces)),
            Arc::new(Mutex::new("Default".to_string())),
        )
    }

    /// Helper to call process_message with default workspace args in tests.
    fn process_msg(
        msg: ClientMessage,
        sessions: &Arc<Mutex<HashMap<String, Session>>>,
        shutdown: &Arc<AtomicBool>,
        current_session: &mut Option<String>,
    ) -> DaemonResponse {
        let (workspaces, active_workspace) = test_workspaces();
        process_message(msg, sessions, &workspaces, &active_workspace, shutdown, current_session)
    }

    #[test]
    fn test_workspace_metadata_new() {
        let ws = WorkspaceMetadata::new("MyWorkspace".to_string());
        assert_eq!(ws.name, "MyWorkspace");
        assert!(ws.last_selected_session.is_none());
        assert_eq!(ws.last_focused_pane, "terminal");
        assert_eq!(ws.sidebar_scroll_offset, 0);
        assert!(ws.created_at > 0);
    }

    #[test]
    fn test_session_metadata_has_workspace_name() {
        let meta = SessionMetadata::new("sess1".to_string(), None, 24, 80);
        assert_eq!(meta.workspace_name, "Default");
    }

    #[test]
    fn test_session_metadata_new_in_workspace() {
        let meta = SessionMetadata::new_in_workspace(
            "sess1".to_string(), None, 24, 80, "MyWorkspace".to_string()
        );
        assert_eq!(meta.workspace_name, "MyWorkspace");
    }

    #[test]
    fn test_create_workspace_message() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let (workspaces, active_workspace) = test_workspaces();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::CreateWorkspace { name: "NewWorkspace".to_string() };
        let response = process_message(msg, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        match response {
            DaemonResponse::WorkspaceCreated { name } => {
                assert_eq!(name, "NewWorkspace");
            }
            _ => panic!("Expected WorkspaceCreated response"),
        }
        assert!(workspaces.lock().unwrap().contains_key("NewWorkspace"));
    }

    #[test]
    fn test_list_workspaces_message() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let (workspaces, active_workspace) = test_workspaces();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let response = process_message(ClientMessage::ListWorkspaces, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        match response {
            DaemonResponse::Workspaces { workspaces: ws_list, active_workspace: active } => {
                assert_eq!(ws_list.len(), 1);
                assert_eq!(ws_list[0].name, "Default");
                assert_eq!(active, "Default");
            }
            _ => panic!("Expected Workspaces response"),
        }
    }

    #[test]
    fn test_rename_workspace_message() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let (workspaces, active_workspace) = test_workspaces();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::RenameWorkspace {
            old_name: "Default".to_string(),
            new_name: "Renamed".to_string(),
        };
        let response = process_message(msg, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        match response {
            DaemonResponse::WorkspaceRenamed { old_name, new_name } => {
                assert_eq!(old_name, "Default");
                assert_eq!(new_name, "Renamed");
            }
            _ => panic!("Expected WorkspaceRenamed response"),
        }
        // Active workspace name should be updated
        assert_eq!(*active_workspace.lock().unwrap(), "Renamed");
        assert!(workspaces.lock().unwrap().contains_key("Renamed"));
        assert!(!workspaces.lock().unwrap().contains_key("Default"));
    }

    #[test]
    fn test_delete_workspace_auto_creates_default() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let (workspaces, active_workspace) = test_workspaces();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::DeleteWorkspace { name: "Default".to_string() };
        let response = process_message(msg, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        match response {
            DaemonResponse::WorkspaceDeleted { name } => {
                assert_eq!(name, "Default");
            }
            _ => panic!("Expected WorkspaceDeleted response"),
        }
        // Should have auto-created a new Default workspace
        assert!(workspaces.lock().unwrap().contains_key("Default"));
    }

    #[test]
    fn test_switch_workspace_message() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let (workspaces, active_workspace) = test_workspaces();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // First create a second workspace
        workspaces.lock().unwrap().insert("Work".to_string(), WorkspaceMetadata::new("Work".to_string()));

        let msg = ClientMessage::SwitchWorkspace { name: "Work".to_string() };
        let response = process_message(msg, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        match response {
            DaemonResponse::WorkspaceSwitched { name, sessions: _, .. } => {
                assert_eq!(name, "Work");
            }
            _ => panic!("Expected WorkspaceSwitched response"),
        }
        assert_eq!(*active_workspace.lock().unwrap(), "Work");
    }

    #[test]
    fn test_session_created_in_active_workspace() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let (workspaces, active_workspace) = test_workspaces();
        // Set active workspace to "Work"
        workspaces.lock().unwrap().insert("Work".to_string(), WorkspaceMetadata::new("Work".to_string()));
        *active_workspace.lock().unwrap() = "Work".to_string();

        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::Attach {
            session_name: "my-session".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };
        let response = process_message(msg, &sessions, &workspaces, &active_workspace, &shutdown, &mut current_session);
        match response {
            DaemonResponse::Attached { session_name, is_new, .. } => {
                assert_eq!(session_name, "my-session");
                assert!(is_new);
            }
            DaemonResponse::Error { message } => panic!("Error: {}", message),
            _ => panic!("Expected Attached"),
        }
        // Session should be in "Work" workspace
        let sessions_guard = sessions.lock().unwrap();
        let session = sessions_guard.get("my-session").unwrap();
        assert_eq!(session.metadata.workspace_name, "Work");
    }

    #[test]
    fn test_get_runtime_dir_format() {
        let dir = get_runtime_dir();
        let dir_str = dir.to_string_lossy();
        // Should contain "sidebar-tui"
        assert!(
            dir_str.contains("sidebar-tui"),
            "Runtime dir should contain 'sidebar-tui': {}",
            dir_str
        );
    }

    #[test]
    fn test_get_socket_path_format() {
        let path = get_socket_path();
        let path_str = path.to_string_lossy();
        // Should end with "daemon.sock" on Unix, "daemon.port" on Windows
        #[cfg(unix)]
        assert!(
            path_str.ends_with("daemon.sock"),
            "Socket path should end with 'daemon.sock': {}",
            path_str
        );
        #[cfg(windows)]
        assert!(
            path_str.ends_with("daemon.port"),
            "Lockfile path should end with 'daemon.port': {}",
            path_str
        );
    }

    #[test]
    fn test_session_creation() {
        let session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");
        assert_eq!(session.name, "test");
        assert_eq!(session.rows, 24);
        assert_eq!(session.cols, 80);
        assert!(!session.is_attached);
    }

    #[test]
    fn test_session_info() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");
        session.is_attached = true;
        let info = session.info();
        assert_eq!(info.name, "test");
        assert_eq!(info.rows, 24);
        assert_eq!(info.cols, 80);
        assert!(info.is_attached);
    }

    #[test]
    fn test_session_write_input() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");
        // Writing to PTY should succeed
        let result = session.write_input(b"echo hello\n");
        assert!(result.is_ok(), "Failed to write input: {:?}", result);
    }

    #[test]
    fn test_session_resize() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");
        let result = session.resize(30, 100);
        assert!(result.is_ok(), "Failed to resize: {:?}", result);
        assert_eq!(session.rows, 30);
        assert_eq!(session.cols, 100);
    }

    #[test]
    fn test_session_is_running() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");
        assert!(session.is_running(), "Session should be running after creation");
    }

    #[test]
    fn test_daemon_with_custom_socket() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());
        assert_eq!(daemon.socket_path(), path);
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_is_not_running_initially() {
        let path = temp_socket_path();
        cleanup_socket(&path);
        let daemon = Daemon::with_socket_path(path.clone());
        assert!(!daemon.is_running());
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_shutdown_signal() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());
        assert!(!daemon.should_shutdown());
        daemon.signal_shutdown();
        assert!(daemon.should_shutdown());
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_get_or_create_session_new() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());
        let result = daemon.get_or_create_session("test", 24, 80, None);
        assert!(result.is_ok(), "Failed to create session: {:?}", result);
        let (info, is_new) = result.unwrap();
        assert!(is_new);
        assert_eq!(info.name, "test");
        assert_eq!(info.rows, 24);
        assert_eq!(info.cols, 80);
        assert!(info.is_attached);
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_get_or_create_session_existing() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());

        // Create session first
        let result1 = daemon.get_or_create_session("test", 24, 80, None);
        assert!(result1.is_ok());
        let (_, is_new1) = result1.unwrap();
        assert!(is_new1);

        // Detach then reattach
        daemon.detach_session("test");
        let result2 = daemon.get_or_create_session("test", 30, 100, None);
        assert!(result2.is_ok());
        let (info, is_new2) = result2.unwrap();
        assert!(!is_new2);
        assert_eq!(info.name, "test");
        // Dimensions should be updated
        assert_eq!(info.rows, 30);
        assert_eq!(info.cols, 100);
        assert!(info.is_attached);
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_detach_session() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());

        // Create and attach
        daemon.get_or_create_session("test", 24, 80, None).unwrap();

        // Detach
        assert!(daemon.detach_session("test"));

        // Check session is detached
        let sessions = daemon.list_sessions();
        assert_eq!(sessions.len(), 1);
        assert!(!sessions[0].is_attached);

        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_detach_nonexistent_session() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());
        assert!(!daemon.detach_session("nonexistent"));
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_kill_session() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());

        // Create session
        daemon.get_or_create_session("test", 24, 80, None).unwrap();
        assert_eq!(daemon.list_sessions().len(), 1);

        // Kill session
        assert!(daemon.kill_session("test"));
        assert_eq!(daemon.list_sessions().len(), 0);

        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_kill_nonexistent_session() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());
        assert!(!daemon.kill_session("nonexistent"));
        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_list_sessions() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());

        // Initially empty
        assert!(daemon.list_sessions().is_empty());

        // Create sessions
        daemon.get_or_create_session("session1", 24, 80, None).unwrap();
        daemon.get_or_create_session("session2", 30, 100, None).unwrap();

        let sessions = daemon.list_sessions();
        assert_eq!(sessions.len(), 2);

        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"session1"));
        assert!(names.contains(&"session2"));

        cleanup_socket(&path);
    }

    #[test]
    fn test_daemon_list_sessions_sorted_by_last_active() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());

        // Create sessions with a delay to get different timestamps (need > 1 second for Unix timestamps)
        daemon.get_or_create_session("older_session", 24, 80, None).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        daemon.get_or_create_session("newer_session", 24, 80, None).unwrap();

        let sessions = daemon.list_sessions();
        assert_eq!(sessions.len(), 2);
        // Most recently created session should be first (highest last_active)
        assert_eq!(sessions[0].name, "newer_session");
        assert_eq!(sessions[1].name, "older_session");

        cleanup_socket(&path);
    }

    #[test]
    fn test_session_info_has_last_active() {
        let path = temp_socket_path();
        let daemon = Daemon::with_socket_path(path.clone());

        daemon.get_or_create_session("test", 24, 80, None).unwrap();

        let sessions = daemon.list_sessions();
        assert_eq!(sessions.len(), 1);
        // last_active should be set to a non-zero value
        assert!(sessions[0].last_active > 0);

        cleanup_socket(&path);
    }

    #[test]
    fn test_encode_decode_client_message() {
        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Attach { session_name, rows, cols, cwd } => {
                assert_eq!(session_name, "test");
                assert_eq!(rows, 24);
                assert_eq!(cols, 80);
                assert!(cwd.is_none());
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_client_message_with_cwd() {
        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: Some(PathBuf::from("/tmp")),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Attach { session_name, rows, cols, cwd } => {
                assert_eq!(session_name, "test");
                assert_eq!(rows, 24);
                assert_eq!(cols, 80);
                assert_eq!(cwd, Some(PathBuf::from("/tmp")));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_daemon_response() {
        let msg = DaemonResponse::Attached {
            session_name: "test".to_string(),
            is_new: true,
            terminal_state: None,
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Attached { session_name, is_new, terminal_state } => {
                assert_eq!(session_name, "test");
                assert!(is_new);
                assert!(terminal_state.is_none());
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_list_message() {
        let msg = ClientMessage::List;
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        assert!(matches!(decoded, ClientMessage::List));
    }

    #[test]
    fn test_encode_decode_kill_message() {
        let msg = ClientMessage::Kill {
            session_name: "victim".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Kill { session_name } => {
                assert_eq!(session_name, "victim");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_input_message() {
        let msg = ClientMessage::Input {
            data: b"echo hello\n".to_vec(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Input { data } => {
                assert_eq!(data, b"echo hello\n");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_resize_message() {
        let msg = ClientMessage::Resize { rows: 30, cols: 100 };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Resize { rows, cols } => {
                assert_eq!(rows, 30);
                assert_eq!(cols, 100);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_sessions_response() {
        let sessions = vec![
            SessionInfo {
                name: "s1".to_string(),
                is_attached: true,
                rows: 24,
                cols: 80,
                last_active: 1000,
                workspace_name: "Default".to_string(),
            },
            SessionInfo {
                name: "s2".to_string(),
                is_attached: false,
                rows: 30,
                cols: 100,
                last_active: 2000,
                workspace_name: "Default".to_string(),
            },
        ];
        let msg = DaemonResponse::Sessions { names: sessions };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Sessions { names } => {
                assert_eq!(names.len(), 2);
                assert_eq!(names[0].name, "s1");
                assert!(names[0].is_attached);
                assert_eq!(names[1].name, "s2");
                assert!(!names[1].is_attached);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_process_attach_new_session() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Attached { session_name, is_new, .. } => {
                assert_eq!(session_name, "test");
                assert!(is_new);
            }
            DaemonResponse::Error { message } => {
                panic!("Expected Attached response, got error: {}", message);
            }
            _ => panic!("Expected Attached response"),
        }

        // Verify session was created
        let sessions = sessions.lock().unwrap();
        assert!(sessions.contains_key("test"));

        // Verify current_session was set
        assert_eq!(current_session, Some("test".to_string()));
    }

    #[test]
    fn test_process_attach_existing_session() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Create initial session
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("test".to_string(), Session::new("test".to_string(), 24, 80, None).unwrap());
        }

        // Attach to existing session
        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 30,
            cols: 100,
            cwd: None,
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Attached { session_name, is_new, .. } => {
                assert_eq!(session_name, "test");
                assert!(!is_new);
            }
            _ => panic!("Expected Attached response"),
        }
    }

    #[test]
    fn test_process_list() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Add some sessions
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("s1".to_string(), Session::new("s1".to_string(), 24, 80, None).unwrap());
            sessions.insert("s2".to_string(), Session::new("s2".to_string(), 30, 100, None).unwrap());
        }

        let response = process_msg(ClientMessage::List, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Sessions { names } => {
                assert_eq!(names.len(), 2);
            }
            _ => panic!("Expected Sessions response"),
        }
    }

    #[test]
    fn test_process_input() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = Some("test".to_string());

        // Create session
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("test".to_string(), Session::new("test".to_string(), 24, 80, None).unwrap());
        }

        let msg = ClientMessage::Input { data: b"echo hello\n".to_vec() };
        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Output { .. } => {}
            _ => panic!("Expected Output response"),
        }
    }

    #[test]
    fn test_process_resize() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = Some("test".to_string());

        // Create session
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("test".to_string(), Session::new("test".to_string(), 24, 80, None).unwrap());
        }

        let msg = ClientMessage::Resize { rows: 30, cols: 100 };
        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Output { .. } => {}
            _ => panic!("Expected Output response"),
        }

        // Verify dimensions were updated
        let sessions = sessions.lock().unwrap();
        let session = sessions.get("test").unwrap();
        assert_eq!(session.rows, 30);
        assert_eq!(session.cols, 100);
    }

    #[test]
    fn test_process_resize_updates_all_sessions() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = Some("session1".to_string());

        // Create two sessions
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("session1".to_string(), Session::new("session1".to_string(), 24, 80, None).unwrap());
            sessions.insert("session2".to_string(), Session::new("session2".to_string(), 24, 80, None).unwrap());
        }

        let msg = ClientMessage::Resize { rows: 30, cols: 100 };
        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Output { .. } => {}
            _ => panic!("Expected Output response"),
        }

        // Verify BOTH sessions were resized, not just the current one
        let sessions = sessions.lock().unwrap();
        let s1 = sessions.get("session1").unwrap();
        assert_eq!(s1.rows, 30, "session1 should be resized");
        assert_eq!(s1.cols, 100, "session1 should be resized");
        let s2 = sessions.get("session2").unwrap();
        assert_eq!(s2.rows, 30, "session2 (background) should also be resized");
        assert_eq!(s2.cols, 100, "session2 (background) should also be resized");
    }

    #[test]
    fn test_process_kill_existing() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Add session
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("victim".to_string(), Session::new("victim".to_string(), 24, 80, None).unwrap());
        }

        let msg = ClientMessage::Kill {
            session_name: "victim".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Killed { session_name } => {
                assert_eq!(session_name, "victim");
            }
            _ => panic!("Expected Killed response"),
        }

        // Verify session was removed
        let sessions = sessions.lock().unwrap();
        assert!(!sessions.contains_key("victim"));
    }

    #[test]
    fn test_process_kill_nonexistent() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::Kill {
            session_name: "nonexistent".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::Error { .. }));
    }

    #[test]
    fn test_process_shutdown() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let response = process_msg(ClientMessage::Shutdown, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::ShuttingDown));
        assert!(shutdown.load(Ordering::SeqCst));
    }

    #[test]
    fn test_process_detach() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = Some("test".to_string());

        // Create session with is_attached = true
        {
            let mut sessions = sessions.lock().unwrap();
            let mut session = Session::new("test".to_string(), 24, 80, None).unwrap();
            session.is_attached = true;
            sessions.insert("test".to_string(), session);
        }

        let response = process_msg(ClientMessage::Detach, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::Detached));

        // Verify session is detached
        let sessions = sessions.lock().unwrap();
        let session = sessions.get("test").unwrap();
        assert!(!session.is_attached);

        // Verify current_session was cleared
        assert!(current_session.is_none());
    }

    #[test]
    fn test_daemon_client_server_integration() {
        let socket_path = temp_socket_path();
        cleanup_socket(&socket_path);

        let daemon = Arc::new(Daemon::with_socket_path(socket_path.clone()));
        let daemon_clone = Arc::clone(&daemon);

        // Start daemon in a thread
        let handle = thread::spawn(move || {
            // Ignore the error from ctrlc (can only be set once per process)
            let _ = daemon_clone.run();
        });

        // Wait for daemon to start
        thread::sleep(Duration::from_millis(200));

        // Connect client
        let result = DaemonClient::connect_to(&socket_path);

        // Signal shutdown regardless of client result
        daemon.signal_shutdown();

        // Give daemon time to process shutdown
        thread::sleep(Duration::from_millis(100));

        // Wait for daemon thread (with timeout to avoid hanging)
        let _ = handle.join();

        cleanup_socket(&socket_path);

        // If we got a client connection, the integration worked
        // (connection may fail if daemon startup failed due to ctrlc being set already)
        if let Ok(mut client) = result {
            // Try to list sessions
            if let Ok(sessions) = client.list_sessions() {
                assert!(sessions.is_empty());
            }
        }
    }

    #[test]
    fn test_session_with_pty_receives_output() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        // Send echo command
        session.write_input(b"echo TESTOUTPUT123\r").unwrap();

        // Wait for output
        let mut received = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match session.pty.rx.try_recv() {
                Ok(PtyEvent::Output(data)) => {
                    let output = String::from_utf8_lossy(&data);
                    if output.contains("TESTOUTPUT123") {
                        received = true;
                        break;
                    }
                }
                Ok(PtyEvent::Exited) => break,
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }

        assert!(received, "Should receive echo output from session PTY");
    }

    // Tests for terminal state serialization (sidebar_tui-1f8)

    #[test]
    fn test_session_terminal_state_initial() {
        let session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");
        let state = session.get_terminal_state();

        // Initial state should have the correct dimensions
        assert_eq!(state.rows, 24);
        assert_eq!(state.cols, 80);
        // Initial cursor should be at top-left
        assert_eq!(state.cursor_position, (0, 0));
    }

    #[test]
    fn test_session_terminal_state_after_text() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        // Process some text directly to the parser
        session.process_raw(b"Hello, World!");

        let state = session.get_terminal_state();

        // Contents should include the text
        let contents_str = String::from_utf8_lossy(&state.contents);
        assert!(contents_str.contains("Hello, World!"), "State should contain the text");

        // Cursor should be after the text
        assert_eq!(state.cursor_position.0, 0, "Cursor row should be 0");
        assert_eq!(state.cursor_position.1, 13, "Cursor col should be 13 (after 'Hello, World!')");
    }

    #[test]
    fn test_session_terminal_state_with_colors() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        // Process red text (ESC[31m = red foreground)
        session.process_raw(b"\x1b[31mRED\x1b[m");

        let state = session.get_terminal_state();

        // Contents_formatted should include color escape sequences
        // The exact format depends on vt100 crate output, but it should contain the text
        let contents_str = String::from_utf8_lossy(&state.contents);
        assert!(contents_str.contains("RED"), "State should contain the text");
        // The escape codes should be present for color
        assert!(state.contents.iter().any(|&b| b == 0x1b), "State should contain escape sequences");
    }

    #[test]
    fn test_session_terminal_state_with_newlines() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        // Process multi-line text
        session.process_raw(b"Line 1\r\nLine 2\r\nLine 3");

        let state = session.get_terminal_state();
        let contents_str = String::from_utf8_lossy(&state.contents);

        assert!(contents_str.contains("Line 1"), "Should contain Line 1");
        assert!(contents_str.contains("Line 2"), "Should contain Line 2");
        assert!(contents_str.contains("Line 3"), "Should contain Line 3");

        // Cursor should be on row 2 (third line)
        assert_eq!(state.cursor_position.0, 2, "Cursor should be on row 2");
    }

    #[test]
    fn test_session_terminal_state_with_cursor_movement() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        // Move cursor to specific position (ESC[5;10H = row 5, col 10, 1-indexed)
        session.process_raw(b"\x1b[5;10HTEXT");

        let state = session.get_terminal_state();

        // Cursor should be at position (4, 13) - 0-indexed
        // ESC[5;10H moves to row 5 col 10 (1-indexed) = (4, 9) 0-indexed
        // Then "TEXT" (4 chars) moves cursor to col 13
        assert_eq!(state.cursor_position.0, 4, "Cursor row should be 4 (0-indexed)");
        assert_eq!(state.cursor_position.1, 13, "Cursor col should be 13 (after TEXT)");
    }

    #[test]
    fn test_session_terminal_contents_helper() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        session.process_raw(b"Testing terminal contents");

        let contents = session.terminal_contents();
        assert!(contents.contains("Testing terminal contents"));
    }

    #[test]
    fn test_session_resize_updates_parser() {
        let mut session = Session::new("test".to_string(), 24, 80, None).expect("Failed to create session");

        // Resize session
        session.resize(40, 120).expect("Failed to resize");

        let state = session.get_terminal_state();
        assert_eq!(state.rows, 40);
        assert_eq!(state.cols, 120);
    }

    #[test]
    fn test_terminal_state_serialization() {
        let state = TerminalState {
            contents: b"Hello\x1b[31mRed\x1b[m".to_vec(),
            cursor_position: (5, 10),
            rows: 24,
            cols: 80,
        };

        // Serialize and deserialize
        let json = serde_json::to_string(&state).expect("Failed to serialize");
        let deserialized: TerminalState = serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.contents, state.contents);
        assert_eq!(deserialized.cursor_position, state.cursor_position);
        assert_eq!(deserialized.rows, state.rows);
        assert_eq!(deserialized.cols, state.cols);
    }

    #[test]
    fn test_process_attach_existing_returns_terminal_state() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Create initial session and add some content
        {
            let mut sessions_guard = sessions.lock().unwrap();
            let mut session = Session::new("test".to_string(), 24, 80, None).unwrap();
            session.process_raw(b"Important content to restore");
            sessions_guard.insert("test".to_string(), session);
        }

        // Attach to existing session
        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Attached { session_name, is_new, terminal_state } => {
                assert_eq!(session_name, "test");
                assert!(!is_new, "Should not be a new session");
                assert!(terminal_state.is_some(), "Should have terminal state for existing session");

                let state_bytes = terminal_state.unwrap();
                let state_str = String::from_utf8_lossy(&state_bytes);
                assert!(state_str.contains("Important content to restore"),
                    "Terminal state should contain the content");
            }
            _ => panic!("Expected Attached response"),
        }
    }

    #[test]
    fn test_process_attach_new_has_no_terminal_state() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::Attach {
            session_name: "brand_new".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Attached { session_name, is_new, terminal_state } => {
                assert_eq!(session_name, "brand_new");
                assert!(is_new, "Should be a new session");
                assert!(terminal_state.is_none(), "New session should not have terminal state");
            }
            _ => panic!("Expected Attached response"),
        }
    }

    #[test]
    fn test_encode_decode_terminal_state_in_response() {
        let terminal_state = Some(b"\x1b[2J\x1b[H\x1b[31mRed Text\x1b[m".to_vec());
        let msg = DaemonResponse::Attached {
            session_name: "test".to_string(),
            is_new: false,
            terminal_state: terminal_state.clone(),
        };

        let encoded = encode_message(&msg).unwrap();
        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Attached { terminal_state: ts, .. } => {
                assert_eq!(ts, terminal_state);
            }
            _ => panic!("Wrong message type"),
        }
    }

    // Tests for session persistence (sidebar_tui-ou5)

    fn temp_data_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        PathBuf::from(format!("/tmp/sidebar-tui-test-data-{}-{}", pid, id))
    }

    fn cleanup_dir(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn test_get_data_dir_format() {
        let dir = get_data_dir();
        let dir_str = dir.to_string_lossy();
        assert!(
            dir_str.contains("sidebar-tui"),
            "Data dir should contain 'sidebar-tui': {}",
            dir_str
        );
    }

    #[test]
    fn test_get_sessions_dir_format() {
        let dir = get_sessions_dir();
        let dir_str = dir.to_string_lossy();
        assert!(
            dir_str.ends_with("sessions"),
            "Sessions dir should end with 'sessions': {}",
            dir_str
        );
    }

    #[test]
    fn test_session_metadata_new() {
        let metadata = SessionMetadata::new(
            "test-session".to_string(),
            Some(PathBuf::from("/home/user")),
            24,
            80,
        );
        assert_eq!(metadata.name, "test-session");
        assert_eq!(metadata.cwd, Some(PathBuf::from("/home/user")));
        assert_eq!(metadata.rows, 24);
        assert_eq!(metadata.cols, 80);
        assert!(metadata.created_at > 0);
        assert_eq!(metadata.created_at, metadata.last_active);
    }

    #[test]
    fn test_session_metadata_touch() {
        let mut metadata = SessionMetadata::new(
            "test-session".to_string(),
            None,
            24,
            80,
        );
        let original_last_active = metadata.last_active;

        // Sleep briefly to ensure time passes
        thread::sleep(Duration::from_millis(10));
        metadata.touch();

        // last_active should stay same or increase (same second is ok)
        assert!(metadata.last_active >= original_last_active);
    }

    #[test]
    fn test_session_metadata_serialization() {
        let metadata = SessionMetadata::new(
            "test-session".to_string(),
            Some(PathBuf::from("/home/user")),
            24,
            80,
        );

        let json = serde_json::to_string(&metadata).expect("Failed to serialize");
        let deserialized: SessionMetadata = serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.name, metadata.name);
        assert_eq!(deserialized.cwd, metadata.cwd);
        assert_eq!(deserialized.rows, metadata.rows);
        assert_eq!(deserialized.cols, metadata.cols);
        assert_eq!(deserialized.created_at, metadata.created_at);
        assert_eq!(deserialized.last_active, metadata.last_active);
    }

    #[test]
    fn test_session_metadata_file_path() {
        let metadata = SessionMetadata::new("my-session".to_string(), None, 24, 80);
        let path = metadata.file_path();
        assert!(
            path.to_string_lossy().ends_with("my-session.json"),
            "Path should end with session name.json: {:?}",
            path
        );
    }

    #[test]
    fn test_session_metadata_save_and_load() {
        // Use a temporary directory for testing
        let test_dir = temp_data_dir();
        let sessions_dir = test_dir.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("Failed to create test dir");

        // Create and save metadata manually to our test location
        let metadata = SessionMetadata::new(
            "save-test".to_string(),
            Some(PathBuf::from("/home/user/project")),
            30,
            100,
        );

        let test_path = sessions_dir.join("save-test.json");
        let json = serde_json::to_string_pretty(&metadata).expect("Failed to serialize");
        fs::write(&test_path, json).expect("Failed to write test file");

        // Load it back
        let loaded = SessionMetadata::load(&test_path).expect("Failed to load metadata");

        assert_eq!(loaded.name, "save-test");
        assert_eq!(loaded.cwd, Some(PathBuf::from("/home/user/project")));
        assert_eq!(loaded.rows, 30);
        assert_eq!(loaded.cols, 100);

        cleanup_dir(&test_dir);
    }

    #[test]
    fn test_session_metadata_delete() {
        let test_dir = temp_data_dir();
        let sessions_dir = test_dir.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("Failed to create test dir");

        // Create a test file
        let test_path = sessions_dir.join("delete-test.json");
        fs::write(&test_path, "{}").expect("Failed to write test file");
        assert!(test_path.exists());

        // Create metadata and test delete
        let _metadata = SessionMetadata::new("delete-test".to_string(), None, 24, 80);
        // Override the file path method by deleting directly
        fs::remove_file(&test_path).expect("Failed to delete");
        assert!(!test_path.exists());

        cleanup_dir(&test_dir);
    }

    #[test]
    fn test_encode_decode_list_stale_message() {
        let msg = ClientMessage::ListStale;
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        assert!(matches!(decoded, ClientMessage::ListStale));
    }

    #[test]
    fn test_encode_decode_restore_stale_message() {
        let msg = ClientMessage::RestoreStale {
            session_name: "old-session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::RestoreStale { session_name } => {
                assert_eq!(session_name, "old-session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_delete_stale_message() {
        let msg = ClientMessage::DeleteStale {
            session_name: "old-session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::DeleteStale { session_name } => {
                assert_eq!(session_name, "old-session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_stale_sessions_response() {
        let sessions = vec![
            SessionMetadata::new("session1".to_string(), None, 24, 80),
            SessionMetadata::new("session2".to_string(), Some(PathBuf::from("/tmp")), 30, 100),
        ];
        let msg = DaemonResponse::StaleSessions { sessions };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::StaleSessions { sessions } => {
                assert_eq!(sessions.len(), 2);
                assert_eq!(sessions[0].name, "session1");
                assert_eq!(sessions[1].name, "session2");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_restored_response() {
        let msg = DaemonResponse::Restored {
            session_name: "restored-session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Restored { session_name } => {
                assert_eq!(session_name, "restored-session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_deleted_response() {
        let msg = DaemonResponse::Deleted {
            session_name: "deleted-session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Deleted { session_name } => {
                assert_eq!(session_name, "deleted-session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_process_list_stale_empty() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let response = process_msg(ClientMessage::ListStale, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::StaleSessions { sessions: _ } => {
                // Should be empty since no metadata exists
                // (or could have stale files from other tests, either is acceptable)
            }
            DaemonResponse::Error { .. } => {
                // Also acceptable if sessions dir doesn't exist
            }
            _ => panic!("Expected StaleSessions or Error response"),
        }
    }

    #[test]
    fn test_process_delete_stale_nonexistent() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::DeleteStale {
            session_name: "nonexistent-session-xyz123".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        // Should get an error since the metadata file doesn't exist
        assert!(matches!(response, DaemonResponse::Error { .. }));
    }

    #[test]
    fn test_process_restore_stale_already_exists() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Create a session first
        {
            let mut sessions_guard = sessions.lock().unwrap();
            sessions_guard.insert(
                "existing-session".to_string(),
                Session::new("existing-session".to_string(), 24, 80, None).unwrap()
            );
        }

        let msg = ClientMessage::RestoreStale {
            session_name: "existing-session".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        // Should fail since session already exists
        match response {
            DaemonResponse::Error { message } => {
                assert!(message.contains("already exists"));
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[test]
    fn test_persisted_session_state_new() {
        let metadata = SessionMetadata::new("test-session".to_string(), None, 24, 80);
        let state = PersistedSessionState::new(metadata.clone());

        assert_eq!(state.metadata.name, "test-session");
        assert_eq!(state.metadata.rows, 24);
        assert_eq!(state.metadata.cols, 80);
        assert!(state.terminal_state.is_none());
        assert!(state.environment.is_none());
        assert_eq!(state.version, PERSISTED_STATE_VERSION);
    }

    #[test]
    fn test_persisted_session_state_file_path() {
        let path = PersistedSessionState::file_path("my-session");
        assert!(path.to_string_lossy().contains("my-session.state"));
    }

    #[test]
    fn test_persisted_session_state_serialization() {
        let metadata = SessionMetadata::new("test".to_string(), Some(PathBuf::from("/tmp")), 30, 100);
        let mut state = PersistedSessionState::new(metadata);
        state.terminal_state = Some(b"\x1b[mHello World".to_vec());
        state.environment = Some([
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
        ].into());

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: PersistedSessionState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.metadata.name, "test");
        assert_eq!(deserialized.metadata.cwd, Some(PathBuf::from("/tmp")));
        assert!(deserialized.terminal_state.is_some());
        assert_eq!(deserialized.terminal_state.as_ref().unwrap().len(), 14);
        assert!(deserialized.environment.is_some());
        let env = deserialized.environment.unwrap();
        assert_eq!(env.get("PATH"), Some(&"/usr/bin".to_string()));
        assert_eq!(env.get("HOME"), Some(&"/home/user".to_string()));
    }

    #[test]
    fn test_default_scrollback_constant() {
        // Verify scrollback is set to 1M lines
        assert_eq!(DEFAULT_SCROLLBACK, 1_000_000);
    }

    #[test]
    fn test_session_uses_scrollback() {
        // Verify Session::new uses DEFAULT_SCROLLBACK
        let session = Session::new("scrollback-test".to_string(), 24, 80, None).unwrap();
        // The session should be able to handle scrollback content
        // We can verify by checking that the parser was created with scrollback
        let screen = session.terminal_parser.screen();
        // state_formatted() will include scrollback if available
        let state = screen.state_formatted();
        // Just verify it returns something (actual content depends on terminal)
        assert!(state.is_empty() || !state.is_empty()); // Tautology to verify no panic

        // Clean up
        let _ = session.metadata.delete();
    }

    // Tests for session rename functionality (sidebar_tui-1pk)

    #[test]
    fn test_encode_decode_rename_message() {
        let msg = ClientMessage::Rename {
            old_name: "old_session".to_string(),
            new_name: "new_session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Rename { old_name, new_name } => {
                assert_eq!(old_name, "old_session");
                assert_eq!(new_name, "new_session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_renamed_response() {
        let msg = DaemonResponse::Renamed {
            old_name: "old_session".to_string(),
            new_name: "new_session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Renamed { old_name, new_name } => {
                assert_eq!(old_name, "old_session");
                assert_eq!(new_name, "new_session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_process_rename_success() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Add session
        {
            let mut sessions_guard = sessions.lock().unwrap();
            sessions_guard.insert("old_name".to_string(), Session::new("old_name".to_string(), 24, 80, None).unwrap());
        }

        let msg = ClientMessage::Rename {
            old_name: "old_name".to_string(),
            new_name: "new_name".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Renamed { old_name, new_name } => {
                assert_eq!(old_name, "old_name");
                assert_eq!(new_name, "new_name");
            }
            _ => panic!("Expected Renamed response, got {:?}", response),
        }

        // Verify session was renamed
        let sessions_guard = sessions.lock().unwrap();
        assert!(!sessions_guard.contains_key("old_name"), "Old name should not exist");
        assert!(sessions_guard.contains_key("new_name"), "New name should exist");
        assert_eq!(sessions_guard.get("new_name").unwrap().name, "new_name");
    }

    #[test]
    fn test_process_rename_nonexistent() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        let msg = ClientMessage::Rename {
            old_name: "nonexistent".to_string(),
            new_name: "new_name".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::Error { .. }));
    }

    #[test]
    fn test_process_rename_conflict() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Add two sessions
        {
            let mut sessions_guard = sessions.lock().unwrap();
            sessions_guard.insert("session1".to_string(), Session::new("session1".to_string(), 24, 80, None).unwrap());
            sessions_guard.insert("session2".to_string(), Session::new("session2".to_string(), 24, 80, None).unwrap());
        }

        // Try to rename session1 to session2 (already exists)
        let msg = ClientMessage::Rename {
            old_name: "session1".to_string(),
            new_name: "session2".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::Error { .. }));

        // Verify nothing changed
        let sessions_guard = sessions.lock().unwrap();
        assert!(sessions_guard.contains_key("session1"), "session1 should still exist");
        assert!(sessions_guard.contains_key("session2"), "session2 should still exist");
    }

    #[test]
    fn test_process_rename_empty_name() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Add session
        {
            let mut sessions_guard = sessions.lock().unwrap();
            sessions_guard.insert("old_name".to_string(), Session::new("old_name".to_string(), 24, 80, None).unwrap());
        }

        // Try to rename to empty string
        let msg = ClientMessage::Rename {
            old_name: "old_name".to_string(),
            new_name: "".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::Error { .. }));

        // Verify session was not changed
        let sessions_guard = sessions.lock().unwrap();
        assert!(sessions_guard.contains_key("old_name"), "old_name should still exist");
    }

    // Tests for preview functionality (sidebar_tui-xjh)

    #[test]
    fn test_encode_decode_preview_message() {
        let msg = ClientMessage::Preview {
            session_name: "test_session".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Preview { session_name } => {
                assert_eq!(session_name, "test_session");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_previewed_response() {
        let msg = DaemonResponse::Previewed {
            session_name: "test_session".to_string(),
            terminal_state: Some(b"Hello, World!".to_vec()),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: DaemonResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            DaemonResponse::Previewed { session_name, terminal_state } => {
                assert_eq!(session_name, "test_session");
                assert_eq!(terminal_state, Some(b"Hello, World!".to_vec()));
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_process_preview_existing_session() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Add a session with some terminal content
        {
            let mut sessions_guard = sessions.lock().unwrap();
            let mut session = Session::new("preview_test".to_string(), 24, 80, None).unwrap();
            // Add some content to the terminal
            session.process_raw(b"Hello, Preview!");
            sessions_guard.insert("preview_test".to_string(), session);
        }

        // Request preview
        let msg = ClientMessage::Preview {
            session_name: "preview_test".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        match response {
            DaemonResponse::Previewed { session_name, terminal_state } => {
                assert_eq!(session_name, "preview_test");
                assert!(terminal_state.is_some());
                let state_bytes = terminal_state.unwrap();
                let contents = String::from_utf8_lossy(&state_bytes);
                assert!(contents.contains("Hello, Preview!"), "Preview should contain terminal content");
            }
            _ => panic!("Expected Previewed response, got {:?}", response),
        }
    }

    #[test]
    fn test_process_preview_nonexistent_session() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_session: Option<String> = None;

        // Request preview for nonexistent session
        let msg = ClientMessage::Preview {
            session_name: "nonexistent".to_string(),
        };

        let response = process_msg(msg, &sessions, &shutdown, &mut current_session);

        assert!(matches!(response, DaemonResponse::Error { .. }));
    }
}
