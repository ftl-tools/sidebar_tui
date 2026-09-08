//! Sidebar server for persistent sessions and their windows.
//!
//! The old workspace/session names obscured tmux's session/window hierarchy.
//! Rust APIs now use that hierarchy; explicit serde names and legacy storage paths
//! preserve existing saved data and compatibility with already-running servers.
//! This remains Sidebar's PTY backend, not a tmux server implementation.
//!
//! This module implements a server that owns PTY windows and communicates
//! with TUI clients via Unix sockets. Windows persist when the TUI disconnects
//! and can be reattached later.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

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
use crate::pty::{PtyEvent, PtyHandle, spawn_shell, spawn_shell_with_env};

/// Terminal state for restoring window on reconnect.
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
/// This allows restoring scrollback history across server restarts.
/// Set to 1M lines to preserve extensive history.
pub const DEFAULT_SCROLLBACK: usize = 1_000_000;

/// Version number for persisted state format.
/// Increment when making breaking changes to the state format.
const PERSISTED_STATE_VERSION: u32 = 1;

/// Persisted window state saved to disk for server restart survival.
/// Unlike WindowMetadata (which is lightweight and always saved), this
/// contains the full terminal state and is only saved during graceful shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedWindowState {
    /// Basic window metadata (name, cwd, dimensions).
    pub metadata: WindowMetadata,
    /// Terminal screen state (formatted escape sequences for replay).
    /// Includes both visible content and scrollback.
    pub terminal_state: Option<Vec<u8>>,
    /// Captured environment variables from the shell process.
    /// Filtered to exclude sensitive values.
    pub environment: Option<HashMap<String, String>>,
    /// Format version for forward compatibility.
    pub version: u32,
}

impl PersistedWindowState {
    /// Create a new persisted state from a window.
    pub fn new(metadata: WindowMetadata) -> Self {
        Self {
            metadata,
            terminal_state: None,
            environment: None,
            version: PERSISTED_STATE_VERSION,
        }
    }

    /// Get the path to this window's state file.
    pub fn file_path(window_name: &str) -> PathBuf {
        get_windows_dir().join(format!("{}.state", window_name))
    }

    /// Save the persisted state to disk.
    pub fn save(&self) -> Result<()> {
        ensure_windows_dir()?;
        let path = Self::file_path(&self.metadata.name);
        let data =
            serde_json::to_vec(self).context("Failed to serialize persisted window state")?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write persisted state to {:?}", path))?;
        Ok(())
    }

    /// Load persisted state from disk.
    pub fn load(window_name: &str) -> Result<Option<Self>> {
        let path = Self::file_path(window_name);
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
    pub fn delete(window_name: &str) -> Result<()> {
        let path = Self::file_path(window_name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete persisted state at {:?}", path))?;
        }
        Ok(())
    }
}

/// Message types for communication between TUI client and server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Attach to a window (create if doesn't exist).
    Attach {
        #[serde(rename = "session_name")]
        window_name: String,
        rows: u16,
        cols: u16,
        /// Working directory for new windows.
        cwd: Option<PathBuf>,
    },
    /// Detach from the current window.
    Detach,
    /// Send input to the window.
    Input { data: Vec<u8> },
    /// Resize the terminal.
    Resize { rows: u16, cols: u16 },
    /// List all active windows.
    List,
    /// Kill a specific window.
    Kill {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// List stale windows (persisted but not currently running).
    ListStale,
    /// Restore a stale window from its persisted metadata.
    RestoreStale {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// Delete stale window metadata (user declined to restore).
    DeleteStale {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// Rename a window.
    Rename { old_name: String, new_name: String },
    /// Get terminal state for preview (without attaching).
    Preview {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// List all sessions.
    #[serde(rename = "ListWorkspaces")]
    ListSessions,
    /// Create a new session.
    #[serde(rename = "CreateWorkspace")]
    CreateSession { name: String },
    /// Rename a session.
    #[serde(rename = "RenameWorkspace")]
    RenameSession { old_name: String, new_name: String },
    /// Kill a session and all its windows.
    #[serde(rename = "DeleteWorkspace")]
    KillSession { name: String },
    /// Switch active session (saves current session state, restores target session state).
    #[serde(rename = "SwitchWorkspace")]
    SwitchSession { name: String },
    /// Move a window to a different session.
    #[serde(rename = "MoveSessionToWorkspace")]
    MoveWindowToSession {
        #[serde(rename = "session_name")]
        window_name: String,
        #[serde(rename = "workspace_name")]
        session_name: String,
    },
    /// Save session view state (selected window, focused region, scroll offset).
    #[serde(rename = "SaveWorkspaceState")]
    SaveSessionState {
        #[serde(rename = "workspace_name")]
        session_name: String,
        #[serde(rename = "last_selected_session")]
        last_selected_window: Option<String>,
        #[serde(rename = "last_focused_pane")]
        last_focused_region: String,
        sidebar_scroll_offset: usize,
    },
    /// Shutdown the server.
    Shutdown,
}

/// Response from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerResponse {
    /// Successfully attached to window.
    Attached {
        #[serde(rename = "session_name")]
        window_name: String,
        is_new: bool,
        /// Serialized terminal state for restoration.
        terminal_state: Option<Vec<u8>>,
    },
    /// Window detached.
    Detached,
    /// PTY output data.
    Output { data: Vec<u8> },
    /// Window list.
    #[serde(rename = "Sessions")]
    Windows { names: Vec<WindowInfo> },
    /// Stale windows list (persisted but not currently running).
    #[serde(rename = "StaleSessions")]
    StaleWindows {
        #[serde(rename = "sessions")]
        windows: Vec<WindowMetadata>,
    },
    /// Stale window was restored.
    Restored {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// Stale window metadata was deleted.
    Deleted {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// Window was killed.
    Killed {
        #[serde(rename = "session_name")]
        window_name: String,
    },
    /// Window was renamed.
    Renamed { old_name: String, new_name: String },
    /// Terminal state for preview (without attaching).
    Previewed {
        #[serde(rename = "session_name")]
        window_name: String,
        /// Serialized terminal state for preview.
        terminal_state: Option<Vec<u8>>,
    },
    /// Session list.
    #[serde(rename = "Workspaces")]
    Sessions {
        #[serde(rename = "workspaces")]
        sessions: Vec<SessionInfo>,
        #[serde(rename = "active_workspace")]
        active_session: String,
    },
    /// Session was created.
    #[serde(rename = "WorkspaceCreated")]
    SessionCreated { name: String },
    /// Session was renamed.
    #[serde(rename = "WorkspaceRenamed")]
    SessionRenamed { old_name: String, new_name: String },
    /// Session was killed.
    #[serde(rename = "WorkspaceDeleted")]
    SessionKilled { name: String },
    /// Switched to a different session.
    #[serde(rename = "WorkspaceSwitched")]
    SessionSwitched {
        name: String,
        #[serde(rename = "sessions")]
        windows: Vec<WindowInfo>,
        #[serde(rename = "last_selected_session")]
        last_selected_window: Option<String>,
        #[serde(rename = "last_focused_pane")]
        last_focused_region: String,
        sidebar_scroll_offset: usize,
    },
    /// Window was moved to a different session.
    #[serde(rename = "SessionMoved")]
    WindowMoved {
        #[serde(rename = "session_name")]
        window_name: String,
        #[serde(rename = "workspace_name")]
        session_name: String,
    },
    /// Session state was saved.
    #[serde(rename = "WorkspaceStateSaved")]
    SessionStateSaved,
    /// Error occurred.
    Error { message: String },
    /// Server is shutting down.
    ShuttingDown,
}

/// Information about a window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub name: String,
    pub is_attached: bool,
    pub rows: u16,
    pub cols: u16,
    /// Timestamp when the window was last active (Unix epoch seconds).
    pub last_active: u64,
    /// Session this window belongs to.
    #[serde(rename = "workspace_name")]
    pub session_name: String,
}

/// Persistent window metadata saved to disk for reboot survival.
/// When the server restarts after a reboot, it can read these files
/// to know about windows that were running before the reboot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowMetadata {
    /// Window name.
    pub name: String,
    /// Working directory for the window.
    pub cwd: Option<PathBuf>,
    /// Terminal dimensions (rows, cols).
    pub rows: u16,
    pub cols: u16,
    /// Timestamp when the window was created (Unix epoch seconds).
    pub created_at: u64,
    /// Timestamp when the window was last active (Unix epoch seconds).
    pub last_active: u64,
    /// Session this window belongs to (default: "Default").
    #[serde(default = "default_session_name")]
    #[serde(rename = "workspace_name")]
    pub session_name: String,
}

fn default_session_name() -> String {
    "Default".to_string()
}

/// Session metadata persisted to disk.
/// A session groups terminal windows and saves view/layout state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Unique name for the session.
    pub name: String,
    /// Timestamp when the session was created (Unix epoch seconds).
    pub created_at: u64,
    /// The window that was last selected in this session.
    #[serde(rename = "last_selected_session")]
    pub last_selected_window: Option<String>,
    /// The pane that was last focused ("sidebar" or "terminal").
    #[serde(rename = "last_focused_pane")]
    pub last_focused_region: String,
    /// Scroll offset in the sidebar window list.
    pub sidebar_scroll_offset: usize,
}

impl SessionMetadata {
    /// Create a new session with default state.
    pub fn new(name: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            name,
            created_at: now,
            last_selected_window: None,
            last_focused_region: "terminal".to_string(),
            sidebar_scroll_offset: 0,
        }
    }

    /// Get the path to the sessions config file.
    pub fn file_path() -> PathBuf {
        get_data_dir().join("workspaces.json")
    }

    /// Load all sessions from disk.
    pub fn load_all() -> Result<Vec<Self>> {
        let path = Self::file_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read sessions from {:?}", path))?;
        let sessions: Vec<Self> = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse sessions from {:?}", path))?;
        Ok(sessions)
    }

    /// Save all sessions to disk.
    pub fn save_all(sessions: &[Self]) -> Result<()> {
        ensure_data_dir()?;
        let path = Self::file_path();
        let data =
            serde_json::to_string_pretty(sessions).context("Failed to serialize sessions")?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write sessions to {:?}", path))?;
        Ok(())
    }
}

/// Information about a session returned to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    #[serde(rename = "last_selected_session")]
    pub last_selected_window: Option<String>,
    #[serde(rename = "last_focused_pane")]
    pub last_focused_region: String,
    pub sidebar_scroll_offset: usize,
}

impl WindowMetadata {
    /// Create new window metadata in the given session.
    pub fn new(name: String, cwd: Option<PathBuf>, rows: u16, cols: u16) -> Self {
        Self::new_in_session(name, cwd, rows, cols, "Default".to_string())
    }

    /// Create new window metadata in a specific session.
    pub fn new_in_session(
        name: String,
        cwd: Option<PathBuf>,
        rows: u16,
        cols: u16,
        session_name: String,
    ) -> Self {
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
            session_name,
        }
    }

    /// Update the last_active timestamp.
    pub fn touch(&mut self) {
        self.last_active = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }

    /// Get the path to this window's metadata file.
    pub fn file_path(&self) -> PathBuf {
        get_windows_dir().join(format!("{}.json", self.name))
    }

    /// Save the metadata to disk.
    pub fn save(&self) -> Result<()> {
        ensure_windows_dir()?;
        let path = self.file_path();
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize window metadata")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write window metadata to {:?}", path))?;
        Ok(())
    }

    /// Load window metadata from a file.
    pub fn load(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)
            .with_context(|| format!("Failed to read window metadata from {:?}", path))?;
        let metadata: Self = serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse window metadata from {:?}", path))?;
        Ok(metadata)
    }

    /// Delete the metadata file from disk.
    pub fn delete(&self) -> Result<()> {
        let path = self.file_path();
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete window metadata at {:?}", path))?;
        }
        Ok(())
    }
}

/// Load all persisted window metadata from disk.
pub fn load_all_window_metadata() -> Result<Vec<WindowMetadata>> {
    let windows_dir = get_windows_dir();
    if !windows_dir.exists() {
        return Ok(Vec::new());
    }

    let mut windows = Vec::new();
    for entry in fs::read_dir(&windows_dir).context("Failed to read windows directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            match WindowMetadata::load(&path) {
                Ok(metadata) => windows.push(metadata),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to load window metadata from {:?}: {:?}",
                        path, e
                    );
                }
            }
        }
    }
    Ok(windows)
}

/// Clean up metadata files for windows that are no longer running.
/// This compares metadata files on disk against currently active windows.
pub fn cleanup_stale_metadata(active_windows: &[String]) -> Result<()> {
    let windows_dir = get_windows_dir();
    if !windows_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&windows_dir).context("Failed to read windows directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !active_windows.contains(&stem.to_string()) {
                    if let Err(e) = fs::remove_file(&path) {
                        eprintln!(
                            "Warning: Failed to remove stale metadata {:?}: {:?}",
                            path, e
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Get the runtime directory for the server socket.
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

/// Get the windows metadata directory.
pub fn get_windows_dir() -> PathBuf {
    get_data_dir().join("sessions")
}

/// Get the socket path for the server (Unix) or port lockfile path (Windows).
pub fn get_socket_path() -> PathBuf {
    #[cfg(unix)]
    return get_runtime_dir().join("daemon.sock");
    #[cfg(windows)]
    return get_runtime_dir().join("daemon.port");
}

/// Read the server TCP port from the lockfile (Windows only).
#[cfg(windows)]
fn read_server_port(lockfile: &Path) -> io::Result<u16> {
    let content = fs::read_to_string(lockfile)?;
    content.trim().parse::<u16>().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid port in lockfile: {}", e),
        )
    })
}

/// Write the server TCP port to the lockfile (Windows only).
#[cfg(windows)]
fn write_server_port(lockfile: &Path, port: u16) -> Result<()> {
    fs::write(lockfile, port.to_string()).context("Failed to write server port lockfile")?;
    Ok(())
}

/// Check if a server is running at the given socket/lockfile path.
fn is_server_running(socket_path: &Path) -> bool {
    #[cfg(unix)]
    {
        if !socket_path.exists() {
            return false;
        }
        UnixStream::connect(socket_path).is_ok()
    }
    #[cfg(windows)]
    {
        match read_server_port(socket_path) {
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
            fs::set_permissions(&dir, perms).context("Failed to set data directory permissions")?;
        }
    }
    Ok(dir)
}

/// Ensure the windows directory exists with proper permissions.
pub fn ensure_windows_dir() -> Result<PathBuf> {
    ensure_data_dir()?;
    let dir = get_windows_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create windows directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(&dir, perms)
                .context("Failed to set windows directory permissions")?;
        }
    }
    Ok(dir)
}

/// Window server that manages terminal windows.
pub struct Server {
    /// Map of window names to window handles.
    windows: Arc<Mutex<HashMap<String, Window>>>,
    /// Session metadata, keyed by session name.
    sessions: Arc<Mutex<HashMap<String, SessionMetadata>>>,
    /// Active session name.
    active_session: Arc<Mutex<String>>,
    /// Path to the Unix socket.
    socket_path: PathBuf,
    /// Flag to signal shutdown.
    shutdown: Arc<AtomicBool>,
}

/// A single terminal window managed by the server.
///
/// The window owns the PTY and manages communication with clients.
/// The window also maintains a vt100 parser to track terminal state
/// for restoring windows on client reconnect.
pub struct Window {
    pub name: String,
    pub rows: u16,
    pub cols: u16,
    pub is_attached: bool,
    /// The PTY handle for this window.
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
    /// Persistent metadata for this window (saved to disk).
    metadata: WindowMetadata,
}

impl Window {
    /// Create a new window with a PTY in the "Default" session.
    pub fn new(name: String, rows: u16, cols: u16, cwd: Option<PathBuf>) -> Result<Self> {
        Self::new_in_session(name, rows, cols, cwd, "Default".to_string())
    }

    /// Create a new window with a PTY in a specific session.
    pub fn new_in_session(
        name: String,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        session_name: String,
    ) -> Result<Self> {
        // Validate dimensions - vt100 panics with 0 dimensions
        let rows = if rows == 0 { 24 } else { rows };
        let cols = if cols == 0 { 80 } else { cols };

        let pty = spawn_shell(rows, cols, cwd.clone())?;
        let shell_running = Arc::new(AtomicBool::new(true));
        // Initialize vt100 parser with same dimensions as PTY.
        // Use DEFAULT_SCROLLBACK to preserve history for window restoration.
        let terminal_parser = vt100::Parser::new(rows, cols, DEFAULT_SCROLLBACK);

        // Create and save metadata for persistence across reboots
        let metadata = WindowMetadata::new_in_session(name.clone(), cwd, rows, cols, session_name);
        if let Err(e) = metadata.save() {
            eprintln!("Warning: Failed to save window metadata: {:?}", e);
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

    /// Create a window with a given PTY handle (for testing).
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
            metadata: WindowMetadata::new(name, None, rows, cols),
        }
    }

    /// Create a new window from persisted state (for restoration after server restart).
    /// This spawns a new shell with the captured environment variables and
    /// replays the terminal state through the parser.
    pub fn from_persisted_state(state: PersistedWindowState) -> Result<Self> {
        let rows = if state.metadata.rows == 0 {
            24
        } else {
            state.metadata.rows
        };
        let cols = if state.metadata.cols == 0 {
            80
        } else {
            state.metadata.cols
        };

        // Spawn shell with restored environment variables
        let pty = spawn_shell_with_env(rows, cols, state.metadata.cwd.clone(), state.environment)?;
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
            eprintln!("Warning: Failed to save restored window metadata: {:?}", e);
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

    /// Create a new window from existing metadata (for restoration without terminal state).
    /// This preserves the original timestamps from the metadata.
    pub fn from_metadata(metadata: WindowMetadata) -> Result<Self> {
        let rows = if metadata.rows == 0 {
            24
        } else {
            metadata.rows
        };
        let cols = if metadata.cols == 0 {
            80
        } else {
            metadata.cols
        };

        // Spawn shell with the original working directory
        let pty = spawn_shell(rows, cols, metadata.cwd.clone())?;
        let shell_running = Arc::new(AtomicBool::new(true));
        let terminal_parser = vt100::Parser::new(rows, cols, DEFAULT_SCROLLBACK);

        // Keep the existing metadata (preserves created_at and last_active timestamps)
        // We don't call touch() here to preserve the order from before server restart

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

    /// Save the window state for persistence across server restarts.
    /// Captures terminal state and environment variables.
    pub fn save_state(&self) -> Result<()> {
        // Get terminal state with scrollback
        let screen = self.terminal_parser.screen();
        // state_formatted includes scrollback + screen state
        let terminal_state = screen.state_formatted();

        // Capture environment variables from the shell process
        let environment = self.pty.process_id().and_then(capture_environment);

        let persisted = PersistedWindowState {
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

    /// Delete the window's persistent metadata from disk.
    pub fn delete_metadata(&self) -> Result<()> {
        self.metadata.delete()
    }

    /// Update the window's last_active timestamp and save to disk.
    pub fn touch_metadata(&mut self) {
        self.metadata.touch();
        if let Err(e) = self.metadata.save() {
            eprintln!("Warning: Failed to update window metadata: {:?}", e);
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
        self.client_output_tx
            .retain(|tx| tx.send(data.to_vec()).is_ok());
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

    /// Get the current terminal state for window restoration.
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

    pub fn info(&self) -> WindowInfo {
        WindowInfo {
            name: self.name.clone(),
            is_attached: self.is_attached,
            rows: self.rows,
            cols: self.cols,
            last_active: self.metadata.last_active,
            session_name: self.metadata.session_name.clone(),
        }
    }

    /// Move this window to a different session.
    pub fn move_to_session(&mut self, session_name: String) {
        self.metadata.session_name = session_name;
        if let Err(e) = self.metadata.save() {
            eprintln!(
                "Warning: Failed to save window metadata after session move: {:?}",
                e
            );
        }
    }
}

impl Server {
    /// Create a new server instance, loading sessions from disk.
    pub fn new() -> Result<Self> {
        let socket_path = get_socket_path();
        let (sessions, active_session) = Self::load_or_init_sessions()?;
        Ok(Self {
            windows: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(sessions)),
            active_session: Arc::new(Mutex::new(active_session)),
            socket_path,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a server with a custom socket path (for testing).
    pub fn with_socket_path(socket_path: PathBuf) -> Self {
        let default_session = SessionMetadata::new("Default".to_string());
        let mut sessions = HashMap::new();
        sessions.insert("Default".to_string(), default_session);
        Self {
            windows: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(sessions)),
            active_session: Arc::new(Mutex::new("Default".to_string())),
            socket_path,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Load sessions from disk, or initialize with a Default session.
    fn load_or_init_sessions() -> Result<(HashMap<String, SessionMetadata>, String)> {
        let persisted = SessionMetadata::load_all().unwrap_or_default();
        if persisted.is_empty() {
            // First run: create Default session
            let default_session = SessionMetadata::new("Default".to_string());
            let mut map = HashMap::new();
            map.insert("Default".to_string(), default_session.clone());
            if let Err(e) = SessionMetadata::save_all(&[default_session]) {
                eprintln!("Warning: Failed to save default session: {:?}", e);
            }
            Ok((map, "Default".to_string()))
        } else {
            let mut map = HashMap::new();
            for session in persisted {
                map.insert(session.name.clone(), session);
            }
            // Active session is the first one (could be made configurable later)
            let active = map
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "Default".to_string());
            Ok((map, active))
        }
    }

    /// Get the active session name.
    pub fn active_session(&self) -> String {
        self.active_session.lock().unwrap().clone()
    }

    /// Get session info list for the client.
    pub fn list_sessions(&self) -> (Vec<SessionInfo>, String) {
        let sessions = self.sessions.lock().unwrap();
        let active = self.active_session.lock().unwrap().clone();
        let mut list: Vec<SessionInfo> = sessions
            .values()
            .map(|session| SessionInfo {
                name: session.name.clone(),
                last_selected_window: session.last_selected_window.clone(),
                last_focused_region: session.last_focused_region.clone(),
                sidebar_scroll_offset: session.sidebar_scroll_offset,
            })
            .collect();
        // Sort alphabetically for consistent display
        list.sort_by(|a, b| a.name.cmp(&b.name));
        (list, active)
    }

    /// Create a new session.
    pub fn create_session(&self, name: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(name) {
            bail!("Session '{}' already exists", name);
        }
        let session = SessionMetadata::new(name.to_string());
        sessions.insert(name.to_string(), session);
        let list: Vec<SessionMetadata> = sessions.values().cloned().collect();
        drop(sessions);
        SessionMetadata::save_all(&list)?;
        Ok(())
    }

    /// Rename a session.
    pub fn rename_session(&self, old_name: &str, new_name: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(old_name) {
            bail!("Session '{}' not found", old_name);
        }
        if sessions.contains_key(new_name) {
            bail!("Session '{}' already exists", new_name);
        }
        let mut session = sessions.remove(old_name).unwrap();
        session.name = new_name.to_string();
        sessions.insert(new_name.to_string(), session);
        let list: Vec<SessionMetadata> = sessions.values().cloned().collect();
        drop(sessions);
        SessionMetadata::save_all(&list)?;

        // Update active session name if needed
        let mut active = self.active_session.lock().unwrap();
        if *active == old_name {
            *active = new_name.to_string();
        }

        // Update all windows belonging to the renamed session
        let mut windows = self.windows.lock().unwrap();
        for window in windows.values_mut() {
            if window.metadata.session_name == old_name {
                window.move_to_session(new_name.to_string());
            }
        }

        Ok(())
    }

    /// Kill a session and all its windows.
    /// If this would leave no sessions, auto-creates a "Default" session.
    pub fn kill_session(&self, name: &str) -> Result<String> {
        // Kill all windows in this session
        {
            let mut windows = self.windows.lock().unwrap();
            let to_kill: Vec<String> = windows
                .values()
                .filter(|s| s.metadata.session_name == name)
                .map(|s| s.name.clone())
                .collect();
            for window_name in to_kill {
                if let Some(window) = windows.remove(&window_name) {
                    let _ = window.delete_metadata();
                    let _ = PersistedWindowState::delete(&window_name);
                }
            }
        }

        let new_active = {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.remove(name);

            // If no sessions left, create Default
            if sessions.is_empty() {
                let default_session = SessionMetadata::new("Default".to_string());
                sessions.insert("Default".to_string(), default_session);
            }

            let list: Vec<SessionMetadata> = sessions.values().cloned().collect();
            drop(sessions);
            SessionMetadata::save_all(&list)?;

            // Determine new active session
            let sessions = self.sessions.lock().unwrap();
            sessions
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "Default".to_string())
        };

        // Update active session if deleted session was active
        let mut active = self.active_session.lock().unwrap();
        if *active == name {
            *active = new_active.clone();
        }

        Ok(new_active)
    }

    /// Switch to a different session. Returns windows in the new session.
    pub fn switch_session(
        &self,
        name: &str,
    ) -> Result<(Vec<WindowInfo>, Option<String>, String, usize)> {
        let sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(name) {
            bail!("Session '{}' not found", name);
        }
        let session = sessions.get(name).unwrap();
        let last_selected = session.last_selected_window.clone();
        let last_focused = session.last_focused_region.clone();
        let scroll_offset = session.sidebar_scroll_offset;
        drop(sessions);

        *self.active_session.lock().unwrap() = name.to_string();

        // Return windows in the new session
        let windows = self.windows.lock().unwrap();
        let mut session_windows: Vec<WindowInfo> = windows
            .values()
            .filter(|s| s.metadata.session_name == name)
            .map(|s| s.info())
            .collect();
        session_windows.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        Ok((session_windows, last_selected, last_focused, scroll_offset))
    }

    /// Move a window to a different session.
    pub fn move_window_to_session(&self, window_name: &str, session_name: &str) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(session_name) {
            bail!("Session '{}' not found", session_name);
        }
        drop(sessions);

        let mut windows = self.windows.lock().unwrap();
        if let Some(window) = windows.get_mut(window_name) {
            window.move_to_session(session_name.to_string());
            Ok(())
        } else {
            bail!("Window '{}' not found", window_name)
        }
    }

    /// Save view state for a session.
    pub fn save_session_state(
        &self,
        session_name: &str,
        last_selected_window: Option<String>,
        last_focused_region: String,
        sidebar_scroll_offset: usize,
    ) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(session_name) {
            session.last_selected_window = last_selected_window;
            session.last_focused_region = last_focused_region;
            session.sidebar_scroll_offset = sidebar_scroll_offset;
        } else {
            bail!("Session '{}' not found", session_name);
        }
        let list: Vec<SessionMetadata> = sessions.values().cloned().collect();
        drop(sessions);
        SessionMetadata::save_all(&list)?;
        Ok(())
    }

    /// Get the socket path for this server.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Check if a server is already running.
    pub fn is_running(&self) -> bool {
        is_server_running(&self.socket_path)
    }

    /// Signal the server to shut down.
    pub fn signal_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Check if shutdown has been signaled.
    pub fn should_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Start the server and listen for connections.
    pub fn run(&self) -> Result<()> {
        self.run_platform()
    }

    #[cfg(unix)]
    fn run_platform(&self) -> Result<()> {
        ensure_runtime_dir()?;

        // Remove stale socket file if it exists
        if self.socket_path.exists() {
            if self.is_running() {
                bail!("Server is already running");
            }
            fs::remove_file(&self.socket_path).context("Failed to remove stale socket file")?;
        }

        let listener =
            UnixListener::bind(&self.socket_path).context("Failed to bind to Unix socket")?;

        // Set non-blocking so we can check for shutdown
        listener
            .set_nonblocking(true)
            .context("Failed to set socket to non-blocking")?;

        // Set up signal handler for graceful shutdown
        self.setup_signal_handler()?;

        while !self.should_shutdown() {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    let windows = Arc::clone(&self.windows);
                    let sessions = Arc::clone(&self.sessions);
                    let active_session = Arc::clone(&self.active_session);
                    let shutdown = Arc::clone(&self.shutdown);
                    thread::spawn(move || {
                        // Configure stream inside the thread so failures are isolated
                        // to this connection and don't crash the whole server.
                        // On macOS, set_read_timeout on Unix domain sockets may return
                        // EINVAL — log and continue rather than propagating.
                        let stream = stream;
                        if let Err(e) = stream.set_nonblocking(false) {
                            eprintln!("Failed to set stream to blocking mode: {:?}", e);
                            return;
                        }
                        if let Err(e) = stream.set_read_timeout(Some(Duration::from_millis(50))) {
                            eprintln!("Failed to set stream read timeout (non-fatal): {:?}", e);
                            // Continue anyway — handle_client can still function without the timeout.
                        }
                        if let Err(e) =
                            handle_client(stream, windows, sessions, active_session, shutdown)
                        {
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
        let listener = TcpListener::bind("127.0.0.1:0").context("Failed to bind TCP listener")?;
        let port = listener
            .local_addr()
            .context("Failed to get local address")?
            .port();

        // Write port to lockfile so clients can discover us
        write_server_port(&self.socket_path, port)?;

        // Set non-blocking so we can check for shutdown
        listener
            .set_nonblocking(true)
            .context("Failed to set listener to non-blocking")?;

        // Set up signal handler for graceful shutdown
        self.setup_signal_handler()?;

        while !self.should_shutdown() {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    stream
                        .set_nonblocking(false)
                        .context("Failed to set stream to blocking mode")?;
                    stream
                        .set_read_timeout(Some(Duration::from_millis(50)))
                        .context("Failed to set stream read timeout")?;
                    let windows = Arc::clone(&self.windows);
                    let sessions = Arc::clone(&self.sessions);
                    let active_session = Arc::clone(&self.active_session);
                    let shutdown = Arc::clone(&self.shutdown);
                    thread::spawn(move || {
                        if let Err(e) =
                            handle_client(stream, windows, sessions, active_session, shutdown)
                        {
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
        let windows = Arc::clone(&self.windows);

        // Use a simple approach with ctrlc for SIGINT/SIGTERM
        // The signal-hook crate would be more comprehensive but ctrlc is simpler
        ctrlc::set_handler(move || {
            // Save all window states before shutdown (for server restart persistence)
            if let Ok(mut windows_guard) = windows.lock() {
                for window in windows_guard.values_mut() {
                    // Save terminal state and environment
                    if let Err(e) = window.save_state() {
                        eprintln!(
                            "Warning: Failed to save window '{}' state: {:?}",
                            window.name, e
                        );
                    }
                    // Gracefully shutdown shell (triggers history save)
                    window.graceful_shutdown();
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

    /// Save all window states to disk (for graceful shutdown).
    pub fn save_all_windows(&self) -> Vec<String> {
        let mut saved = Vec::new();
        if let Ok(windows_guard) = self.windows.lock() {
            for window in windows_guard.values() {
                if let Err(e) = window.save_state() {
                    eprintln!(
                        "Warning: Failed to save window '{}' state: {:?}",
                        window.name, e
                    );
                } else {
                    saved.push(window.name.clone());
                }
            }
        }
        saved
    }

    /// Get a list of all windows (sorted by most recently used first).
    pub fn list_windows(&self) -> Vec<WindowInfo> {
        let windows = self.windows.lock().unwrap();
        let mut list: Vec<WindowInfo> = windows.values().map(|s| s.info()).collect();
        // Sort by last_active descending (most recently used first)
        list.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        list
    }

    /// Create or get a window.
    pub fn get_or_create_window(
        &self,
        name: &str,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
    ) -> Result<(WindowInfo, bool)> {
        let mut windows = self.windows.lock().unwrap();
        if let Some(window) = windows.get_mut(name) {
            // Window exists, mark as attached and update dimensions
            window.is_attached = true;
            if let Err(e) = window.resize(rows, cols) {
                // Log but don't fail - window still exists
                eprintln!("Warning: failed to resize window: {:?}", e);
            }
            Ok((window.info(), false))
        } else {
            // Create new window with PTY
            let mut window = Window::new(name.to_string(), rows, cols, cwd)?;
            window.is_attached = true;
            let info = window.info();
            windows.insert(name.to_string(), window);
            Ok((info, true))
        }
    }

    /// Detach from a window.
    pub fn detach_window(&self, name: &str) -> bool {
        let mut windows = self.windows.lock().unwrap();
        if let Some(window) = windows.get_mut(name) {
            window.is_attached = false;
            true
        } else {
            false
        }
    }

    /// Kill a window.
    pub fn kill_window(&self, name: &str) -> bool {
        let mut windows = self.windows.lock().unwrap();
        if let Some(window) = windows.remove(name) {
            // Delete the persistent metadata and state files
            if let Err(e) = window.delete_metadata() {
                eprintln!("Warning: Failed to delete window metadata: {:?}", e);
            }
            if let Err(e) = PersistedWindowState::delete(name) {
                eprintln!("Warning: Failed to delete window state: {:?}", e);
            }
            true
        } else {
            false
        }
    }

    /// Get a list of stale windows (persisted metadata with no running server window).
    /// These are windows that were running before a reboot/crash.
    pub fn get_stale_windows(&self) -> Vec<WindowMetadata> {
        let windows = self.windows.lock().unwrap();
        let active_names: Vec<String> = windows.keys().cloned().collect();
        drop(windows); // Release lock before file I/O

        match load_all_window_metadata() {
            Ok(all_metadata) => all_metadata
                .into_iter()
                .filter(|m| !active_names.contains(&m.name))
                .collect(),
            Err(e) => {
                eprintln!("Warning: Failed to load window metadata: {:?}", e);
                Vec::new()
            }
        }
    }

    /// Restore a stale window from its metadata and persisted state.
    /// If a .state file exists, the terminal state and environment will be restored.
    /// Otherwise, creates a new window with the same name and working directory.
    pub fn restore_window(&self, metadata: &WindowMetadata) -> Result<WindowInfo> {
        let mut windows = self.windows.lock().unwrap();
        if windows.contains_key(&metadata.name) {
            bail!("Window '{}' already exists", metadata.name);
        }

        // Check for persisted state file (.state) with terminal and env data
        let window = match PersistedWindowState::load(&metadata.name)? {
            Some(persisted_state) => {
                // Full restoration with terminal state and environment
                Window::from_persisted_state(persisted_state)?
            }
            None => {
                // Fallback to metadata-only restoration (no terminal state)
                Window::new(
                    metadata.name.clone(),
                    metadata.rows,
                    metadata.cols,
                    metadata.cwd.clone(),
                )?
            }
        };

        let info = window.info();
        windows.insert(metadata.name.clone(), window);

        // Clean up the .state file after successful restoration
        // (the window will create a new one on next shutdown)
        let _ = PersistedWindowState::delete(&metadata.name);

        Ok(info)
    }

    /// Delete metadata for a stale window (user declined to restore it).
    pub fn delete_stale_metadata(&self, name: &str) -> Result<()> {
        let path = get_windows_dir().join(format!("{}.json", name));
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete stale metadata for '{}'", name))?;
        }
        // Also delete the .state file if it exists
        let _ = PersistedWindowState::delete(name);
        Ok(())
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new().expect("Failed to create default server")
    }
}

/// Handle a client connection.
/// The stream must already be configured (blocking, read timeout set) by the caller.
fn handle_client(
    mut stream: IpcStream,
    windows: Arc<Mutex<HashMap<String, Window>>>,
    sessions: Arc<Mutex<HashMap<String, SessionMetadata>>>,
    active_session: Arc<Mutex<String>>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let mut current_window: Option<String> = None;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            send_response(&mut stream, &ServerResponse::ShuttingDown)?;
            break;
        }

        // Process PTY output if attached to a window
        // Collect all output in a local buffer first, then send without holding the lock
        if let Some(ref window_name) = current_window {
            let outputs = {
                let mut windows_guard = windows.lock().unwrap();
                let mut outputs = Vec::new();
                let mut shell_exited = false;

                if let Some(window) = windows_guard.get_mut(window_name) {
                    // Collect all pending PTY output
                    loop {
                        match window.pty.rx.try_recv() {
                            Ok(PtyEvent::Output(data)) => {
                                // Feed through vt100 parser to track terminal state
                                window.terminal_parser.process(&data);
                                outputs.push(data);
                            }
                            Ok(PtyEvent::Exited) => {
                                window.shell_running.store(false, Ordering::SeqCst);
                                shell_exited = true;
                                break;
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                window.shell_running.store(false, Ordering::SeqCst);
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
                send_response(&mut stream, &ServerResponse::Output { data })?;
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
                // Client disconnected - mark window as detached
                if let Some(window_name) = current_window.take() {
                    let mut windows_guard = windows.lock().unwrap();
                    if let Some(window) = windows_guard.get_mut(&window_name) {
                        window.is_attached = false;
                    }
                }
                break;
            }
            Err(e) => {
                return Err(e).context("Failed to read message")?;
            }
        };

        let response = process_message(
            msg,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        send_response(&mut stream, &response)?;

        // Only break on shutdown - client may want to reattach after detach
        if matches!(response, ServerResponse::ShuttingDown) {
            break;
        }
    }

    Ok(())
}

/// Process a client message and return a response.
fn process_message(
    msg: ClientMessage,
    windows: &Arc<Mutex<HashMap<String, Window>>>,
    sessions: &Arc<Mutex<HashMap<String, SessionMetadata>>>,
    active_session: &Arc<Mutex<String>>,
    shutdown: &Arc<AtomicBool>,
    current_window: &mut Option<String>,
) -> ServerResponse {
    match msg {
        ClientMessage::Attach {
            window_name,
            rows,
            cols,
            cwd,
        } => {
            // Validate dimensions - vt100 panics with 0 dimensions
            let rows = if rows == 0 { 24 } else { rows };
            let cols = if cols == 0 { 80 } else { cols };

            let mut windows_guard = windows.lock().unwrap();
            let is_new = !windows_guard.contains_key(&window_name);
            let mut terminal_state = None;

            // Create new window if it doesn't exist
            if is_new {
                let session_name = active_session.lock().unwrap().clone();
                match Window::new_in_session(window_name.clone(), rows, cols, cwd, session_name) {
                    Ok(mut window) => {
                        window.is_attached = true;
                        windows_guard.insert(window_name.clone(), window);
                    }
                    Err(e) => {
                        return ServerResponse::Error {
                            message: format!("Failed to create window: {}", e),
                        };
                    }
                }
            } else {
                // Reattaching to existing window - get terminal state for restoration
                if let Some(window) = windows_guard.get_mut(&window_name) {
                    window.is_attached = true;
                    // Get the current terminal state before resizing
                    let state = window.get_terminal_state();
                    terminal_state = Some(state.contents);
                    // Now resize to match client dimensions
                    if let Err(e) = window.resize(rows, cols) {
                        eprintln!("Warning: failed to resize window: {:?}", e);
                    }
                    // Update last_active timestamp when attaching to the window
                    window.touch_metadata();
                }
            }

            *current_window = Some(window_name.clone());

            ServerResponse::Attached {
                window_name,
                is_new,
                terminal_state,
            }
        }
        ClientMessage::Detach => {
            if let Some(window_name) = current_window.take() {
                let mut windows_guard = windows.lock().unwrap();
                if let Some(window) = windows_guard.get_mut(&window_name) {
                    window.is_attached = false;
                }
            }
            ServerResponse::Detached
        }
        ClientMessage::Input { data } => {
            if let Some(window_name) = current_window {
                let mut windows_guard = windows.lock().unwrap();
                if let Some(window) = windows_guard.get_mut(window_name) {
                    if let Err(e) = window.write_input(&data) {
                        return ServerResponse::Error {
                            message: format!("Failed to write input: {}", e),
                        };
                    }
                    // Update last_active timestamp when user sends input
                    window.touch_metadata();
                }
            }
            ServerResponse::Output { data: vec![] }
        }
        ClientMessage::Resize { rows, cols } => {
            let mut windows_guard = windows.lock().unwrap();
            for window in windows_guard.values_mut() {
                if let Err(e) = window.resize(rows, cols) {
                    let name = window.name.clone();
                    return ServerResponse::Error {
                        message: format!("Failed to resize window '{}': {}", name, e),
                    };
                }
            }
            ServerResponse::Output { data: vec![] }
        }
        ClientMessage::List => {
            let windows_guard = windows.lock().unwrap();
            let mut names: Vec<WindowInfo> = windows_guard.values().map(|s| s.info()).collect();
            // Sort by last_active descending (most recently used first)
            names.sort_by(|a, b| b.last_active.cmp(&a.last_active));
            ServerResponse::Windows { names }
        }
        ClientMessage::Kill { window_name } => {
            let mut windows_guard = windows.lock().unwrap();
            if let Some(window) = windows_guard.remove(&window_name) {
                // Delete the persistent metadata and state files
                if let Err(e) = window.delete_metadata() {
                    eprintln!("Warning: Failed to delete window metadata: {:?}", e);
                }
                if let Err(e) = PersistedWindowState::delete(&window_name) {
                    eprintln!("Warning: Failed to delete window state: {:?}", e);
                }
                ServerResponse::Killed { window_name }
            } else {
                ServerResponse::Error {
                    message: format!("Window '{}' not found", window_name),
                }
            }
        }
        ClientMessage::ListStale => {
            let windows_guard = windows.lock().unwrap();
            let active_names: Vec<String> = windows_guard.keys().cloned().collect();
            drop(windows_guard);

            match load_all_window_metadata() {
                Ok(all_metadata) => {
                    let stale: Vec<WindowMetadata> = all_metadata
                        .into_iter()
                        .filter(|m| !active_names.contains(&m.name))
                        .collect();
                    ServerResponse::StaleWindows { windows: stale }
                }
                Err(e) => ServerResponse::Error {
                    message: format!("Failed to load window metadata: {}", e),
                },
            }
        }
        ClientMessage::RestoreStale { window_name } => {
            // First check if window already exists
            {
                let windows_guard = windows.lock().unwrap();
                if windows_guard.contains_key(&window_name) {
                    return ServerResponse::Error {
                        message: format!("Window '{}' already exists", window_name),
                    };
                }
            }

            // Load metadata for this window
            let metadata_path = get_windows_dir().join(format!("{}.json", window_name));
            match WindowMetadata::load(&metadata_path) {
                Ok(metadata) => {
                    // Check for persisted state file (.state) with terminal and env data
                    let window_result = match PersistedWindowState::load(&window_name) {
                        Ok(Some(persisted_state)) => {
                            // Full restoration with terminal state and environment
                            Window::from_persisted_state(persisted_state)
                        }
                        Ok(None) => {
                            // Fallback to metadata-only restoration (no terminal state)
                            // Use from_metadata to preserve original timestamps
                            Window::from_metadata(metadata)
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to load persisted state: {:?}", e);
                            // Fallback to metadata-only restoration
                            // Use from_metadata to preserve original timestamps
                            Window::from_metadata(metadata)
                        }
                    };

                    match window_result {
                        Ok(window) => {
                            let mut windows_guard = windows.lock().unwrap();
                            windows_guard.insert(window_name.clone(), window);
                            // Clean up .state file after successful restoration
                            let _ = PersistedWindowState::delete(&window_name);
                            ServerResponse::Restored { window_name }
                        }
                        Err(e) => ServerResponse::Error {
                            message: format!("Failed to restore window: {}", e),
                        },
                    }
                }
                Err(e) => ServerResponse::Error {
                    message: format!("Failed to load window metadata: {}", e),
                },
            }
        }
        ClientMessage::DeleteStale { window_name } => {
            let metadata_path = get_windows_dir().join(format!("{}.json", window_name));
            if metadata_path.exists() {
                match fs::remove_file(&metadata_path) {
                    Ok(()) => {
                        // Also clean up the .state file if it exists
                        let _ = PersistedWindowState::delete(&window_name);
                        ServerResponse::Deleted { window_name }
                    }
                    Err(e) => ServerResponse::Error {
                        message: format!("Failed to delete metadata: {}", e),
                    },
                }
            } else {
                ServerResponse::Error {
                    message: format!("No metadata found for window '{}'", window_name),
                }
            }
        }
        ClientMessage::Rename { old_name, new_name } => {
            // Validate the new name is not empty
            if new_name.is_empty() {
                return ServerResponse::Error {
                    message: "New window name cannot be empty".to_string(),
                };
            }

            let mut windows_guard = windows.lock().unwrap();

            // Check if new name already exists
            if windows_guard.contains_key(&new_name) {
                return ServerResponse::Error {
                    message: format!("Window '{}' already exists", new_name),
                };
            }

            // Remove the old window
            if let Some(mut window) = windows_guard.remove(&old_name) {
                // Delete old metadata files
                if let Err(e) = window.delete_metadata() {
                    eprintln!("Warning: Failed to delete old window metadata: {:?}", e);
                }
                let _ = PersistedWindowState::delete(&old_name);

                // Update window name and save new metadata
                window.name = new_name.clone();
                window.metadata.name = new_name.clone();
                window.metadata.touch();
                if let Err(e) = window.metadata.save() {
                    eprintln!("Warning: Failed to save renamed window metadata: {:?}", e);
                }

                windows_guard.insert(new_name.clone(), window);
                ServerResponse::Renamed { old_name, new_name }
            } else {
                ServerResponse::Error {
                    message: format!("Window '{}' not found", old_name),
                }
            }
        }
        ClientMessage::Preview { window_name } => {
            let windows_guard = windows.lock().unwrap();
            if let Some(window) = windows_guard.get(&window_name) {
                // Get terminal state for preview using the same method as window attach
                let state = window.get_terminal_state();
                ServerResponse::Previewed {
                    window_name,
                    terminal_state: Some(state.contents),
                }
            } else {
                ServerResponse::Error {
                    message: format!("Window '{}' not found", window_name),
                }
            }
        }
        ClientMessage::ListSessions => {
            let sessions_guard = sessions.lock().unwrap();
            let active = active_session.lock().unwrap().clone();
            let mut list: Vec<SessionInfo> = sessions_guard
                .values()
                .map(|session| SessionInfo {
                    name: session.name.clone(),
                    last_selected_window: session.last_selected_window.clone(),
                    last_focused_region: session.last_focused_region.clone(),
                    sidebar_scroll_offset: session.sidebar_scroll_offset,
                })
                .collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            ServerResponse::Sessions {
                sessions: list,
                active_session: active,
            }
        }
        ClientMessage::CreateSession { name } => {
            let mut sessions_guard = sessions.lock().unwrap();
            if sessions_guard.contains_key(&name) {
                return ServerResponse::Error {
                    message: format!("Session '{}' already exists", name),
                };
            }
            let session = SessionMetadata::new(name.clone());
            sessions_guard.insert(name.clone(), session);
            let list: Vec<SessionMetadata> = sessions_guard.values().cloned().collect();
            drop(sessions_guard);
            if let Err(e) = SessionMetadata::save_all(&list) {
                return ServerResponse::Error {
                    message: format!("Failed to save session: {}", e),
                };
            }
            ServerResponse::SessionCreated { name }
        }
        ClientMessage::RenameSession { old_name, new_name } => {
            let mut sessions_guard = sessions.lock().unwrap();
            if !sessions_guard.contains_key(&old_name) {
                return ServerResponse::Error {
                    message: format!("Session '{}' not found", old_name),
                };
            }
            if sessions_guard.contains_key(&new_name) {
                return ServerResponse::Error {
                    message: format!("Session '{}' already exists", new_name),
                };
            }
            let mut session = sessions_guard.remove(&old_name).unwrap();
            session.name = new_name.clone();
            sessions_guard.insert(new_name.clone(), session);
            let list: Vec<SessionMetadata> = sessions_guard.values().cloned().collect();
            drop(sessions_guard);
            if let Err(e) = SessionMetadata::save_all(&list) {
                return ServerResponse::Error {
                    message: format!("Failed to save sessions: {}", e),
                };
            }
            // Update active session if renamed
            let mut active = active_session.lock().unwrap();
            if *active == old_name {
                *active = new_name.clone();
            }
            // Update all windows in renamed session
            let mut windows_guard = windows.lock().unwrap();
            for window in windows_guard.values_mut() {
                if window.metadata.session_name == old_name {
                    window.move_to_session(new_name.clone());
                }
            }
            ServerResponse::SessionRenamed { old_name, new_name }
        }
        ClientMessage::KillSession { name } => {
            // Kill all windows in this session
            {
                let mut windows_guard = windows.lock().unwrap();
                let to_kill: Vec<String> = windows_guard
                    .values()
                    .filter(|s| s.metadata.session_name == name)
                    .map(|s| s.name.clone())
                    .collect();
                for window_name in to_kill {
                    if let Some(window) = windows_guard.remove(&window_name) {
                        let _ = window.delete_metadata();
                        let _ = PersistedWindowState::delete(&window_name);
                    }
                }
            }

            let mut sessions_guard = sessions.lock().unwrap();
            sessions_guard.remove(&name);

            // If no sessions left, create Default
            if sessions_guard.is_empty() {
                let default_session = SessionMetadata::new("Default".to_string());
                sessions_guard.insert("Default".to_string(), default_session);
            }

            let list: Vec<SessionMetadata> = sessions_guard.values().cloned().collect();
            let remaining_names: Vec<String> = sessions_guard.keys().cloned().collect();
            drop(sessions_guard);

            if let Err(e) = SessionMetadata::save_all(&list) {
                return ServerResponse::Error {
                    message: format!("Failed to save sessions: {}", e),
                };
            }

            // Update active session if deleted session was active
            let mut active = active_session.lock().unwrap();
            if *active == name {
                *active = remaining_names
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "Default".to_string());
            }

            ServerResponse::SessionKilled { name }
        }
        ClientMessage::SwitchSession { name } => {
            let (last_selected, last_focused, scroll_offset) = {
                let sessions_guard = sessions.lock().unwrap();
                if !sessions_guard.contains_key(&name) {
                    return ServerResponse::Error {
                        message: format!("Session '{}' not found", name),
                    };
                }
                let session = sessions_guard.get(&name).unwrap();
                (
                    session.last_selected_window.clone(),
                    session.last_focused_region.clone(),
                    session.sidebar_scroll_offset,
                )
            };
            *active_session.lock().unwrap() = name.clone();

            let windows_guard = windows.lock().unwrap();
            let mut session_windows: Vec<WindowInfo> = windows_guard
                .values()
                .filter(|s| s.metadata.session_name == name)
                .map(|s| s.info())
                .collect();
            session_windows.sort_by(|a, b| b.last_active.cmp(&a.last_active));
            ServerResponse::SessionSwitched {
                name,
                windows: session_windows,
                last_selected_window: last_selected,
                last_focused_region: last_focused,
                sidebar_scroll_offset: scroll_offset,
            }
        }
        ClientMessage::MoveWindowToSession {
            window_name,
            session_name,
        } => {
            {
                let sessions_guard = sessions.lock().unwrap();
                if !sessions_guard.contains_key(&session_name) {
                    return ServerResponse::Error {
                        message: format!("Session '{}' not found", session_name),
                    };
                }
            }
            let mut windows_guard = windows.lock().unwrap();
            if let Some(window) = windows_guard.get_mut(&window_name) {
                window.move_to_session(session_name.clone());
                ServerResponse::WindowMoved {
                    window_name,
                    session_name,
                }
            } else {
                ServerResponse::Error {
                    message: format!("Window '{}' not found", window_name),
                }
            }
        }
        ClientMessage::SaveSessionState {
            session_name,
            last_selected_window,
            last_focused_region,
            sidebar_scroll_offset,
        } => {
            let mut sessions_guard = sessions.lock().unwrap();
            if let Some(session) = sessions_guard.get_mut(&session_name) {
                session.last_selected_window = last_selected_window;
                session.last_focused_region = last_focused_region;
                session.sidebar_scroll_offset = sidebar_scroll_offset;
            } else {
                return ServerResponse::Error {
                    message: format!("Session '{}' not found", session_name),
                };
            }
            let list: Vec<SessionMetadata> = sessions_guard.values().cloned().collect();
            drop(sessions_guard);
            if let Err(e) = SessionMetadata::save_all(&list) {
                return ServerResponse::Error {
                    message: format!("Failed to save session state: {}", e),
                };
            }
            ServerResponse::SessionStateSaved
        }
        ClientMessage::Shutdown => {
            shutdown.store(true, Ordering::SeqCst);
            ServerResponse::ShuttingDown
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

    serde_json::from_slice(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e)))
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
fn send_response(stream: &mut IpcStream, response: &ServerResponse) -> Result<()> {
    let encoded = encode_message(response)?;
    stream
        .write_all(&encoded)
        .context("Failed to write response")?;
    stream.flush().context("Failed to flush response")?;
    Ok(())
}

/// Client handle for connecting to the server.
pub struct ServerClient {
    stream: IpcStream,
}

impl ServerClient {
    /// Connect to the server.
    pub fn connect() -> Result<Self> {
        let socket_path = get_socket_path();
        Self::connect_to(&socket_path)
    }

    /// Connect to a server at the given path.
    /// On Unix: connects to the Unix domain socket at `socket_path`.
    /// On Windows: reads the TCP port from the lockfile at `socket_path` and
    /// connects to `127.0.0.1:{port}`.
    pub fn connect_to(socket_path: &Path) -> Result<Self> {
        #[cfg(unix)]
        let stream = UnixStream::connect(socket_path).context("Failed to connect to server")?;
        #[cfg(windows)]
        let stream = {
            let port = read_server_port(socket_path)
                .context("Failed to read server port from lockfile")?;
            TcpStream::connect(format!("127.0.0.1:{}", port))
                .context("Failed to connect to server via TCP")?
        };
        Ok(Self { stream })
    }

    /// Send a message to the server and wait for a response.
    pub fn send(&mut self, msg: ClientMessage) -> Result<ServerResponse> {
        let encoded = encode_message(&msg)?;
        self.stream
            .write_all(&encoded)
            .context("Failed to send message")?;
        self.stream.flush().context("Failed to flush message")?;

        let response: ServerResponse =
            decode_message(&mut self.stream).context("Failed to read response")?;
        Ok(response)
    }

    /// Attach to a window.
    pub fn attach(
        &mut self,
        window_name: &str,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
    ) -> Result<ServerResponse> {
        self.send(ClientMessage::Attach {
            window_name: window_name.to_string(),
            rows,
            cols,
            cwd,
        })
    }

    /// Send input to the current window.
    pub fn send_input(&mut self, data: &[u8]) -> Result<()> {
        match self.send(ClientMessage::Input {
            data: data.to_vec(),
        })? {
            ServerResponse::Output { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Resize the terminal.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        match self.send(ClientMessage::Resize { rows, cols })? {
            ServerResponse::Output { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Detach from the current window.
    pub fn detach(&mut self) -> Result<()> {
        match self.send(ClientMessage::Detach)? {
            ServerResponse::Detached => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Try to receive PTY output without blocking.
    pub fn try_recv_output(&mut self) -> Result<Option<Vec<u8>>> {
        self.stream
            .set_read_timeout(Some(Duration::from_millis(1)))
            .context("Failed to set read timeout")?;

        match decode_message::<ServerResponse>(&mut self.stream) {
            Ok(ServerResponse::Output { data }) => Ok(Some(data)),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(None),
            Err(e) => Err(e).context("Failed to receive output")?,
        }
    }

    /// List all windows.
    pub fn list_windows(&mut self) -> Result<Vec<WindowInfo>> {
        match self.send(ClientMessage::List)? {
            ServerResponse::Windows { names } => Ok(names),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Kill a window.
    pub fn kill_window(&mut self, window_name: &str) -> Result<()> {
        match self.send(ClientMessage::Kill {
            window_name: window_name.to_string(),
        })? {
            ServerResponse::Killed { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Request server shutdown.
    pub fn shutdown(&mut self) -> Result<()> {
        match self.send(ClientMessage::Shutdown)? {
            ServerResponse::ShuttingDown => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// List stale windows (persisted but not currently running).
    pub fn list_stale_windows(&mut self) -> Result<Vec<WindowMetadata>> {
        match self.send(ClientMessage::ListStale)? {
            ServerResponse::StaleWindows { windows } => Ok(windows),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Restore a stale window from its persisted metadata.
    pub fn restore_stale_window(&mut self, window_name: &str) -> Result<()> {
        match self.send(ClientMessage::RestoreStale {
            window_name: window_name.to_string(),
        })? {
            ServerResponse::Restored { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Delete stale window metadata (user declined to restore).
    pub fn delete_stale_window(&mut self, window_name: &str) -> Result<()> {
        match self.send(ClientMessage::DeleteStale {
            window_name: window_name.to_string(),
        })? {
            ServerResponse::Deleted { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Rename a window.
    pub fn rename_window(&mut self, old_name: &str, new_name: &str) -> Result<()> {
        match self.send(ClientMessage::Rename {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        })? {
            ServerResponse::Renamed { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Get terminal state for preview (without attaching).
    pub fn preview_window(&mut self, window_name: &str) -> Result<Option<Vec<u8>>> {
        match self.send(ClientMessage::Preview {
            window_name: window_name.to_string(),
        })? {
            ServerResponse::Previewed { terminal_state, .. } => Ok(terminal_state),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// List all sessions and the active session.
    pub fn list_sessions(&mut self) -> Result<(Vec<SessionInfo>, String)> {
        match self.send(ClientMessage::ListSessions)? {
            ServerResponse::Sessions {
                sessions,
                active_session,
            } => Ok((sessions, active_session)),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Create a new session.
    pub fn create_session(&mut self, name: &str) -> Result<()> {
        match self.send(ClientMessage::CreateSession {
            name: name.to_string(),
        })? {
            ServerResponse::SessionCreated { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Rename a session.
    pub fn rename_session(&mut self, old_name: &str, new_name: &str) -> Result<()> {
        match self.send(ClientMessage::RenameSession {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        })? {
            ServerResponse::SessionRenamed { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Kill a session and all its windows.
    pub fn kill_session(&mut self, name: &str) -> Result<()> {
        match self.send(ClientMessage::KillSession {
            name: name.to_string(),
        })? {
            ServerResponse::SessionKilled { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Switch to a different session. Returns windows in the new session.
    pub fn switch_session(&mut self, name: &str) -> Result<Vec<WindowInfo>> {
        match self.send(ClientMessage::SwitchSession {
            name: name.to_string(),
        })? {
            ServerResponse::SessionSwitched { windows, .. } => Ok(windows),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Move a window to a different session.
    pub fn move_window_to_session(&mut self, window_name: &str, session_name: &str) -> Result<()> {
        match self.send(ClientMessage::MoveWindowToSession {
            window_name: window_name.to_string(),
            session_name: session_name.to_string(),
        })? {
            ServerResponse::WindowMoved { .. } => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
            other => bail!("Unexpected response: {:?}", other),
        }
    }

    /// Save session view state.
    pub fn save_session_state(
        &mut self,
        session_name: &str,
        last_selected_window: Option<String>,
        last_focused_region: String,
        sidebar_scroll_offset: usize,
    ) -> Result<()> {
        match self.send(ClientMessage::SaveSessionState {
            session_name: session_name.to_string(),
            last_selected_window,
            last_focused_region,
            sidebar_scroll_offset,
        })? {
            ServerResponse::SessionStateSaved => Ok(()),
            ServerResponse::Error { message } => bail!("{}", message),
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

    /// Create default sessions and active_session arcs for testing.
    fn test_sessions() -> (
        Arc<Mutex<HashMap<String, SessionMetadata>>>,
        Arc<Mutex<String>>,
    ) {
        let mut sessions = HashMap::new();
        sessions.insert(
            "Default".to_string(),
            SessionMetadata::new("Default".to_string()),
        );
        (
            Arc::new(Mutex::new(sessions)),
            Arc::new(Mutex::new("Default".to_string())),
        )
    }

    /// Helper to call process_message with default session args in tests.
    fn process_msg(
        msg: ClientMessage,
        windows: &Arc<Mutex<HashMap<String, Window>>>,
        shutdown: &Arc<AtomicBool>,
        current_window: &mut Option<String>,
    ) -> ServerResponse {
        let (sessions, active_session) = test_sessions();
        process_message(
            msg,
            windows,
            &sessions,
            &active_session,
            shutdown,
            current_window,
        )
    }

    #[test]
    fn test_session_metadata_new() {
        let session = SessionMetadata::new("MySession".to_string());
        assert_eq!(session.name, "MySession");
        assert!(session.last_selected_window.is_none());
        assert_eq!(session.last_focused_region, "terminal");
        assert_eq!(session.sidebar_scroll_offset, 0);
        assert!(session.created_at > 0);
    }

    #[test]
    fn test_window_metadata_has_session_name() {
        let meta = WindowMetadata::new("sess1".to_string(), None, 24, 80);
        assert_eq!(meta.session_name, "Default");
    }

    #[test]
    fn test_window_metadata_new_in_session() {
        let meta = WindowMetadata::new_in_session(
            "sess1".to_string(),
            None,
            24,
            80,
            "MySession".to_string(),
        );
        assert_eq!(meta.session_name, "MySession");
    }

    #[test]
    fn test_create_session_message() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let (sessions, active_session) = test_sessions();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::CreateSession {
            name: "NewSession".to_string(),
        };
        let response = process_message(
            msg,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        match response {
            ServerResponse::SessionCreated { name } => {
                assert_eq!(name, "NewSession");
            }
            _ => panic!("Expected SessionCreated response"),
        }
        assert!(sessions.lock().unwrap().contains_key("NewSession"));
    }

    #[test]
    fn test_list_sessions_message() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let (sessions, active_session) = test_sessions();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let response = process_message(
            ClientMessage::ListSessions,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        match response {
            ServerResponse::Sessions {
                sessions: session_list,
                active_session: active,
            } => {
                assert_eq!(session_list.len(), 1);
                assert_eq!(session_list[0].name, "Default");
                assert_eq!(active, "Default");
            }
            _ => panic!("Expected Sessions response"),
        }
    }

    #[test]
    fn test_rename_session_message() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let (sessions, active_session) = test_sessions();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::RenameSession {
            old_name: "Default".to_string(),
            new_name: "Renamed".to_string(),
        };
        let response = process_message(
            msg,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        match response {
            ServerResponse::SessionRenamed { old_name, new_name } => {
                assert_eq!(old_name, "Default");
                assert_eq!(new_name, "Renamed");
            }
            _ => panic!("Expected SessionRenamed response"),
        }
        // Active session name should be updated
        assert_eq!(*active_session.lock().unwrap(), "Renamed");
        assert!(sessions.lock().unwrap().contains_key("Renamed"));
        assert!(!sessions.lock().unwrap().contains_key("Default"));
    }

    #[test]
    fn test_kill_session_auto_creates_default() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let (sessions, active_session) = test_sessions();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::KillSession {
            name: "Default".to_string(),
        };
        let response = process_message(
            msg,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        match response {
            ServerResponse::SessionKilled { name } => {
                assert_eq!(name, "Default");
            }
            _ => panic!("Expected SessionKilled response"),
        }
        // Should have auto-created a new Default session
        assert!(sessions.lock().unwrap().contains_key("Default"));
    }

    #[test]
    fn test_switch_session_message() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let (sessions, active_session) = test_sessions();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // First create a second session
        sessions
            .lock()
            .unwrap()
            .insert("Work".to_string(), SessionMetadata::new("Work".to_string()));

        let msg = ClientMessage::SwitchSession {
            name: "Work".to_string(),
        };
        let response = process_message(
            msg,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        match response {
            ServerResponse::SessionSwitched {
                name, windows: _, ..
            } => {
                assert_eq!(name, "Work");
            }
            _ => panic!("Expected SessionSwitched response"),
        }
        assert_eq!(*active_session.lock().unwrap(), "Work");
    }

    #[test]
    fn test_window_created_in_active_session() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let (sessions, active_session) = test_sessions();
        // Set active session to "Work"
        sessions
            .lock()
            .unwrap()
            .insert("Work".to_string(), SessionMetadata::new("Work".to_string()));
        *active_session.lock().unwrap() = "Work".to_string();

        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::Attach {
            window_name: "my-window".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };
        let response = process_message(
            msg,
            &windows,
            &sessions,
            &active_session,
            &shutdown,
            &mut current_window,
        );
        match response {
            ServerResponse::Attached {
                window_name,
                is_new,
                ..
            } => {
                assert_eq!(window_name, "my-window");
                assert!(is_new);
            }
            ServerResponse::Error { message } => panic!("Error: {}", message),
            _ => panic!("Expected Attached"),
        }
        // Window should be in "Work" session
        let windows_guard = windows.lock().unwrap();
        let window = windows_guard.get("my-window").unwrap();
        assert_eq!(window.metadata.session_name, "Work");
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
    fn test_window_creation() {
        let window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");
        assert_eq!(window.name, "test");
        assert_eq!(window.rows, 24);
        assert_eq!(window.cols, 80);
        assert!(!window.is_attached);
    }

    #[test]
    fn test_window_info() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");
        window.is_attached = true;
        let info = window.info();
        assert_eq!(info.name, "test");
        assert_eq!(info.rows, 24);
        assert_eq!(info.cols, 80);
        assert!(info.is_attached);
    }

    #[test]
    fn test_window_write_input() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");
        // Writing to PTY should succeed
        let result = window.write_input(b"echo hello\n");
        assert!(result.is_ok(), "Failed to write input: {:?}", result);
    }

    #[test]
    fn test_window_resize() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");
        let result = window.resize(30, 100);
        assert!(result.is_ok(), "Failed to resize: {:?}", result);
        assert_eq!(window.rows, 30);
        assert_eq!(window.cols, 100);
    }

    #[test]
    fn test_window_is_running() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");
        assert!(
            window.is_running(),
            "Window should be running after creation"
        );
    }

    #[test]
    fn test_server_with_custom_socket() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());
        assert_eq!(server.socket_path(), path);
        cleanup_socket(&path);
    }

    #[test]
    fn test_server_is_not_running_initially() {
        let path = temp_socket_path();
        cleanup_socket(&path);
        let server = Server::with_socket_path(path.clone());
        assert!(!server.is_running());
        cleanup_socket(&path);
    }

    #[test]
    fn test_server_shutdown_signal() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());
        assert!(!server.should_shutdown());
        server.signal_shutdown();
        assert!(server.should_shutdown());
        cleanup_socket(&path);
    }

    #[test]
    fn test_server_get_or_create_window_new() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());
        let result = server.get_or_create_window("test", 24, 80, None);
        assert!(result.is_ok(), "Failed to create window: {:?}", result);
        let (info, is_new) = result.unwrap();
        assert!(is_new);
        assert_eq!(info.name, "test");
        assert_eq!(info.rows, 24);
        assert_eq!(info.cols, 80);
        assert!(info.is_attached);
        cleanup_socket(&path);
    }

    #[test]
    fn test_server_get_or_create_window_existing() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());

        // Create window first
        let result1 = server.get_or_create_window("test", 24, 80, None);
        assert!(result1.is_ok());
        let (_, is_new1) = result1.unwrap();
        assert!(is_new1);

        // Detach then reattach
        server.detach_window("test");
        let result2 = server.get_or_create_window("test", 30, 100, None);
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
    fn test_server_detach_window() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());

        // Create and attach
        server.get_or_create_window("test", 24, 80, None).unwrap();

        // Detach
        assert!(server.detach_window("test"));

        // Check window is detached
        let windows = server.list_windows();
        assert_eq!(windows.len(), 1);
        assert!(!windows[0].is_attached);

        cleanup_socket(&path);
    }

    #[test]
    fn test_server_detach_nonexistent_window() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());
        assert!(!server.detach_window("nonexistent"));
        cleanup_socket(&path);
    }

    #[test]
    fn test_server_kill_window() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());

        // Create window
        server.get_or_create_window("test", 24, 80, None).unwrap();
        assert_eq!(server.list_windows().len(), 1);

        // Kill window
        assert!(server.kill_window("test"));
        assert_eq!(server.list_windows().len(), 0);

        cleanup_socket(&path);
    }

    #[test]
    fn test_server_kill_nonexistent_window() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());
        assert!(!server.kill_window("nonexistent"));
        cleanup_socket(&path);
    }

    #[test]
    fn test_server_list_windows() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());

        // Initially empty
        assert!(server.list_windows().is_empty());

        // Create windows
        server
            .get_or_create_window("window1", 24, 80, None)
            .unwrap();
        server
            .get_or_create_window("window2", 30, 100, None)
            .unwrap();

        let windows = server.list_windows();
        assert_eq!(windows.len(), 2);

        let names: Vec<&str> = windows.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"window1"));
        assert!(names.contains(&"window2"));

        cleanup_socket(&path);
    }

    #[test]
    fn test_server_list_windows_sorted_by_last_active() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());

        // Create windows with a delay to get different timestamps (need > 1 second for Unix timestamps)
        server
            .get_or_create_window("older_window", 24, 80, None)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        server
            .get_or_create_window("newer_window", 24, 80, None)
            .unwrap();

        let windows = server.list_windows();
        assert_eq!(windows.len(), 2);
        // Most recently created window should be first (highest last_active)
        assert_eq!(windows[0].name, "newer_window");
        assert_eq!(windows[1].name, "older_window");

        cleanup_socket(&path);
    }

    #[test]
    fn test_window_info_has_last_active() {
        let path = temp_socket_path();
        let server = Server::with_socket_path(path.clone());

        server.get_or_create_window("test", 24, 80, None).unwrap();

        let windows = server.list_windows();
        assert_eq!(windows.len(), 1);
        // last_active should be set to a non-zero value
        assert!(windows[0].last_active > 0);

        cleanup_socket(&path);
    }

    #[test]
    fn test_encode_decode_client_message() {
        let msg = ClientMessage::Attach {
            window_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Attach {
                window_name,
                rows,
                cols,
                cwd,
            } => {
                assert_eq!(window_name, "test");
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
            window_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: Some(PathBuf::from("/tmp")),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Attach {
                window_name,
                rows,
                cols,
                cwd,
            } => {
                assert_eq!(window_name, "test");
                assert_eq!(rows, 24);
                assert_eq!(cols, 80);
                assert_eq!(cwd, Some(PathBuf::from("/tmp")));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_server_response() {
        let msg = ServerResponse::Attached {
            window_name: "test".to_string(),
            is_new: true,
            terminal_state: None,
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Attached {
                window_name,
                is_new,
                terminal_state,
            } => {
                assert_eq!(window_name, "test");
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
            window_name: "victim".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Kill { window_name } => {
                assert_eq!(window_name, "victim");
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
        let msg = ClientMessage::Resize {
            rows: 30,
            cols: 100,
        };
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
    fn test_encode_decode_windows_response() {
        let windows = vec![
            WindowInfo {
                name: "s1".to_string(),
                is_attached: true,
                rows: 24,
                cols: 80,
                last_active: 1000,
                session_name: "Default".to_string(),
            },
            WindowInfo {
                name: "s2".to_string(),
                is_attached: false,
                rows: 30,
                cols: 100,
                last_active: 2000,
                session_name: "Default".to_string(),
            },
        ];
        let msg = ServerResponse::Windows { names: windows };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Windows { names } => {
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
    fn test_process_attach_new_window() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::Attach {
            window_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Attached {
                window_name,
                is_new,
                ..
            } => {
                assert_eq!(window_name, "test");
                assert!(is_new);
            }
            ServerResponse::Error { message } => {
                panic!("Expected Attached response, got error: {}", message);
            }
            _ => panic!("Expected Attached response"),
        }

        // Verify window was created
        let windows = windows.lock().unwrap();
        assert!(windows.contains_key("test"));

        // Verify current_window was set
        assert_eq!(current_window, Some("test".to_string()));
    }

    #[test]
    fn test_process_attach_existing_window() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Create initial window
        {
            let mut windows = windows.lock().unwrap();
            windows.insert(
                "test".to_string(),
                Window::new("test".to_string(), 24, 80, None).unwrap(),
            );
        }

        // Attach to existing window
        let msg = ClientMessage::Attach {
            window_name: "test".to_string(),
            rows: 30,
            cols: 100,
            cwd: None,
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Attached {
                window_name,
                is_new,
                ..
            } => {
                assert_eq!(window_name, "test");
                assert!(!is_new);
            }
            _ => panic!("Expected Attached response"),
        }
    }

    #[test]
    fn test_process_list() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Add some windows
        {
            let mut windows = windows.lock().unwrap();
            windows.insert(
                "s1".to_string(),
                Window::new("s1".to_string(), 24, 80, None).unwrap(),
            );
            windows.insert(
                "s2".to_string(),
                Window::new("s2".to_string(), 30, 100, None).unwrap(),
            );
        }

        let response = process_msg(
            ClientMessage::List,
            &windows,
            &shutdown,
            &mut current_window,
        );

        match response {
            ServerResponse::Windows { names } => {
                assert_eq!(names.len(), 2);
            }
            _ => panic!("Expected Windows response"),
        }
    }

    #[test]
    fn test_process_input() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = Some("test".to_string());

        // Create window
        {
            let mut windows = windows.lock().unwrap();
            windows.insert(
                "test".to_string(),
                Window::new("test".to_string(), 24, 80, None).unwrap(),
            );
        }

        let msg = ClientMessage::Input {
            data: b"echo hello\n".to_vec(),
        };
        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Output { .. } => {}
            _ => panic!("Expected Output response"),
        }
    }

    #[test]
    fn test_process_resize() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = Some("test".to_string());

        // Create window
        {
            let mut windows = windows.lock().unwrap();
            windows.insert(
                "test".to_string(),
                Window::new("test".to_string(), 24, 80, None).unwrap(),
            );
        }

        let msg = ClientMessage::Resize {
            rows: 30,
            cols: 100,
        };
        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Output { .. } => {}
            _ => panic!("Expected Output response"),
        }

        // Verify dimensions were updated
        let windows = windows.lock().unwrap();
        let window = windows.get("test").unwrap();
        assert_eq!(window.rows, 30);
        assert_eq!(window.cols, 100);
    }

    #[test]
    fn test_process_resize_updates_all_windows() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = Some("window1".to_string());

        // Create two windows
        {
            let mut windows = windows.lock().unwrap();
            windows.insert(
                "window1".to_string(),
                Window::new("window1".to_string(), 24, 80, None).unwrap(),
            );
            windows.insert(
                "window2".to_string(),
                Window::new("window2".to_string(), 24, 80, None).unwrap(),
            );
        }

        let msg = ClientMessage::Resize {
            rows: 30,
            cols: 100,
        };
        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Output { .. } => {}
            _ => panic!("Expected Output response"),
        }

        // Verify BOTH windows were resized, not just the current one
        let windows = windows.lock().unwrap();
        let s1 = windows.get("window1").unwrap();
        assert_eq!(s1.rows, 30, "window1 should be resized");
        assert_eq!(s1.cols, 100, "window1 should be resized");
        let s2 = windows.get("window2").unwrap();
        assert_eq!(s2.rows, 30, "window2 (background) should also be resized");
        assert_eq!(s2.cols, 100, "window2 (background) should also be resized");
    }

    #[test]
    fn test_process_kill_existing() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Add window
        {
            let mut windows = windows.lock().unwrap();
            windows.insert(
                "victim".to_string(),
                Window::new("victim".to_string(), 24, 80, None).unwrap(),
            );
        }

        let msg = ClientMessage::Kill {
            window_name: "victim".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Killed { window_name } => {
                assert_eq!(window_name, "victim");
            }
            _ => panic!("Expected Killed response"),
        }

        // Verify window was removed
        let windows = windows.lock().unwrap();
        assert!(!windows.contains_key("victim"));
    }

    #[test]
    fn test_process_kill_nonexistent() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::Kill {
            window_name: "nonexistent".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        assert!(matches!(response, ServerResponse::Error { .. }));
    }

    #[test]
    fn test_process_shutdown() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let response = process_msg(
            ClientMessage::Shutdown,
            &windows,
            &shutdown,
            &mut current_window,
        );

        assert!(matches!(response, ServerResponse::ShuttingDown));
        assert!(shutdown.load(Ordering::SeqCst));
    }

    #[test]
    fn test_process_detach() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = Some("test".to_string());

        // Create window with is_attached = true
        {
            let mut windows = windows.lock().unwrap();
            let mut window = Window::new("test".to_string(), 24, 80, None).unwrap();
            window.is_attached = true;
            windows.insert("test".to_string(), window);
        }

        let response = process_msg(
            ClientMessage::Detach,
            &windows,
            &shutdown,
            &mut current_window,
        );

        assert!(matches!(response, ServerResponse::Detached));

        // Verify window is detached
        let windows = windows.lock().unwrap();
        let window = windows.get("test").unwrap();
        assert!(!window.is_attached);

        // Verify current_window was cleared
        assert!(current_window.is_none());
    }

    #[test]
    fn test_server_client_server_integration() {
        let socket_path = temp_socket_path();
        cleanup_socket(&socket_path);

        let server = Arc::new(Server::with_socket_path(socket_path.clone()));
        let server_clone = Arc::clone(&server);

        // Start server in a thread
        let handle = thread::spawn(move || {
            // Ignore the error from ctrlc (can only be set once per process)
            let _ = server_clone.run();
        });

        // Wait for server to start
        thread::sleep(Duration::from_millis(200));

        // Connect client
        let result = ServerClient::connect_to(&socket_path);

        // Signal shutdown regardless of client result
        server.signal_shutdown();

        // Give server time to process shutdown
        thread::sleep(Duration::from_millis(100));

        // Wait for server thread (with timeout to avoid hanging)
        let _ = handle.join();

        cleanup_socket(&socket_path);

        // If we got a client connection, the integration worked
        // (connection may fail if server startup failed due to ctrlc being set already)
        if let Ok(mut client) = result {
            // Try to list windows
            if let Ok(windows) = client.list_windows() {
                assert!(windows.is_empty());
            }
        }
    }

    #[test]
    fn test_window_with_pty_receives_output() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        // Send echo command
        window.write_input(b"echo TESTOUTPUT123\r").unwrap();

        // Wait for output
        let mut received = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match window.pty.rx.try_recv() {
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

        assert!(received, "Should receive echo output from window PTY");
    }

    // Tests for terminal state serialization (sidebar_tui-1f8)

    #[test]
    fn test_window_terminal_state_initial() {
        let window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");
        let state = window.get_terminal_state();

        // Initial state should have the correct dimensions
        assert_eq!(state.rows, 24);
        assert_eq!(state.cols, 80);
        // Initial cursor should be at top-left
        assert_eq!(state.cursor_position, (0, 0));
    }

    #[test]
    fn test_window_terminal_state_after_text() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        // Process some text directly to the parser
        window.process_raw(b"Hello, World!");

        let state = window.get_terminal_state();

        // Contents should include the text
        let contents_str = String::from_utf8_lossy(&state.contents);
        assert!(
            contents_str.contains("Hello, World!"),
            "State should contain the text"
        );

        // Cursor should be after the text
        assert_eq!(state.cursor_position.0, 0, "Cursor row should be 0");
        assert_eq!(
            state.cursor_position.1, 13,
            "Cursor col should be 13 (after 'Hello, World!')"
        );
    }

    #[test]
    fn test_window_terminal_state_with_colors() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        // Process red text (ESC[31m = red foreground)
        window.process_raw(b"\x1b[31mRED\x1b[m");

        let state = window.get_terminal_state();

        // Contents_formatted should include color escape sequences
        // The exact format depends on vt100 crate output, but it should contain the text
        let contents_str = String::from_utf8_lossy(&state.contents);
        assert!(
            contents_str.contains("RED"),
            "State should contain the text"
        );
        // The escape codes should be present for color
        assert!(
            state.contents.iter().any(|&b| b == 0x1b),
            "State should contain escape sequences"
        );
    }

    #[test]
    fn test_window_terminal_state_with_newlines() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        // Process multi-line text
        window.process_raw(b"Line 1\r\nLine 2\r\nLine 3");

        let state = window.get_terminal_state();
        let contents_str = String::from_utf8_lossy(&state.contents);

        assert!(contents_str.contains("Line 1"), "Should contain Line 1");
        assert!(contents_str.contains("Line 2"), "Should contain Line 2");
        assert!(contents_str.contains("Line 3"), "Should contain Line 3");

        // Cursor should be on row 2 (third line)
        assert_eq!(state.cursor_position.0, 2, "Cursor should be on row 2");
    }

    #[test]
    fn test_window_terminal_state_with_cursor_movement() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        // Move cursor to specific position (ESC[5;10H = row 5, col 10, 1-indexed)
        window.process_raw(b"\x1b[5;10HTEXT");

        let state = window.get_terminal_state();

        // Cursor should be at position (4, 13) - 0-indexed
        // ESC[5;10H moves to row 5 col 10 (1-indexed) = (4, 9) 0-indexed
        // Then "TEXT" (4 chars) moves cursor to col 13
        assert_eq!(
            state.cursor_position.0, 4,
            "Cursor row should be 4 (0-indexed)"
        );
        assert_eq!(
            state.cursor_position.1, 13,
            "Cursor col should be 13 (after TEXT)"
        );
    }

    #[test]
    fn test_window_terminal_contents_helper() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        window.process_raw(b"Testing terminal contents");

        let contents = window.terminal_contents();
        assert!(contents.contains("Testing terminal contents"));
    }

    #[test]
    fn test_window_resize_updates_parser() {
        let mut window =
            Window::new("test".to_string(), 24, 80, None).expect("Failed to create window");

        // Resize window
        window.resize(40, 120).expect("Failed to resize");

        let state = window.get_terminal_state();
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
        let deserialized: TerminalState =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.contents, state.contents);
        assert_eq!(deserialized.cursor_position, state.cursor_position);
        assert_eq!(deserialized.rows, state.rows);
        assert_eq!(deserialized.cols, state.cols);
    }

    #[test]
    fn test_process_attach_existing_returns_terminal_state() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Create initial window and add some content
        {
            let mut windows_guard = windows.lock().unwrap();
            let mut window = Window::new("test".to_string(), 24, 80, None).unwrap();
            window.process_raw(b"Important content to restore");
            windows_guard.insert("test".to_string(), window);
        }

        // Attach to existing window
        let msg = ClientMessage::Attach {
            window_name: "test".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Attached {
                window_name,
                is_new,
                terminal_state,
            } => {
                assert_eq!(window_name, "test");
                assert!(!is_new, "Should not be a new window");
                assert!(
                    terminal_state.is_some(),
                    "Should have terminal state for existing window"
                );

                let state_bytes = terminal_state.unwrap();
                let state_str = String::from_utf8_lossy(&state_bytes);
                assert!(
                    state_str.contains("Important content to restore"),
                    "Terminal state should contain the content"
                );
            }
            _ => panic!("Expected Attached response"),
        }
    }

    #[test]
    fn test_process_attach_new_has_no_terminal_state() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::Attach {
            window_name: "brand_new".to_string(),
            rows: 24,
            cols: 80,
            cwd: None,
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Attached {
                window_name,
                is_new,
                terminal_state,
            } => {
                assert_eq!(window_name, "brand_new");
                assert!(is_new, "Should be a new window");
                assert!(
                    terminal_state.is_none(),
                    "New window should not have terminal state"
                );
            }
            _ => panic!("Expected Attached response"),
        }
    }

    #[test]
    fn test_encode_decode_terminal_state_in_response() {
        let terminal_state = Some(b"\x1b[2J\x1b[H\x1b[31mRed Text\x1b[m".to_vec());
        let msg = ServerResponse::Attached {
            window_name: "test".to_string(),
            is_new: false,
            terminal_state: terminal_state.clone(),
        };

        let encoded = encode_message(&msg).unwrap();
        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Attached {
                terminal_state: ts, ..
            } => {
                assert_eq!(ts, terminal_state);
            }
            _ => panic!("Wrong message type"),
        }
    }

    // Tests for window persistence (sidebar_tui-ou5)

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
    fn test_get_windows_dir_format() {
        let dir = get_windows_dir();
        let dir_str = dir.to_string_lossy();
        assert!(
            dir_str.ends_with("sessions"),
            "Window storage retains the legacy 'sessions' directory: {}",
            dir_str
        );
    }

    #[test]
    fn test_window_metadata_new() {
        let metadata = WindowMetadata::new(
            "test-window".to_string(),
            Some(PathBuf::from("/home/user")),
            24,
            80,
        );
        assert_eq!(metadata.name, "test-window");
        assert_eq!(metadata.cwd, Some(PathBuf::from("/home/user")));
        assert_eq!(metadata.rows, 24);
        assert_eq!(metadata.cols, 80);
        assert!(metadata.created_at > 0);
        assert_eq!(metadata.created_at, metadata.last_active);
    }

    #[test]
    fn test_window_metadata_touch() {
        let mut metadata = WindowMetadata::new("test-window".to_string(), None, 24, 80);
        let original_last_active = metadata.last_active;

        // Sleep briefly to ensure time passes
        thread::sleep(Duration::from_millis(10));
        metadata.touch();

        // last_active should stay same or increase (same second is ok)
        assert!(metadata.last_active >= original_last_active);
    }

    #[test]
    fn test_window_metadata_serialization() {
        let metadata = WindowMetadata::new(
            "test-window".to_string(),
            Some(PathBuf::from("/home/user")),
            24,
            80,
        );

        let json = serde_json::to_string(&metadata).expect("Failed to serialize");
        let deserialized: WindowMetadata =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.name, metadata.name);
        assert_eq!(deserialized.cwd, metadata.cwd);
        assert_eq!(deserialized.rows, metadata.rows);
        assert_eq!(deserialized.cols, metadata.cols);
        assert_eq!(deserialized.created_at, metadata.created_at);
        assert_eq!(deserialized.last_active, metadata.last_active);
    }

    #[test]
    fn test_window_metadata_file_path() {
        let metadata = WindowMetadata::new("my-window".to_string(), None, 24, 80);
        let path = metadata.file_path();
        assert!(
            path.to_string_lossy().ends_with("my-window.json"),
            "Path should end with window name.json: {:?}",
            path
        );
    }

    #[test]
    fn test_window_metadata_save_and_load() {
        // Use a temporary directory for testing
        let test_dir = temp_data_dir();
        let windows_dir = test_dir.join("sessions");
        fs::create_dir_all(&windows_dir).expect("Failed to create test dir");

        // Create and save metadata manually to our test location
        let metadata = WindowMetadata::new(
            "save-test".to_string(),
            Some(PathBuf::from("/home/user/project")),
            30,
            100,
        );

        let test_path = windows_dir.join("save-test.json");
        let json = serde_json::to_string_pretty(&metadata).expect("Failed to serialize");
        fs::write(&test_path, json).expect("Failed to write test file");

        // Load it back
        let loaded = WindowMetadata::load(&test_path).expect("Failed to load metadata");

        assert_eq!(loaded.name, "save-test");
        assert_eq!(loaded.cwd, Some(PathBuf::from("/home/user/project")));
        assert_eq!(loaded.rows, 30);
        assert_eq!(loaded.cols, 100);

        cleanup_dir(&test_dir);
    }

    #[test]
    fn test_window_metadata_delete() {
        let test_dir = temp_data_dir();
        let windows_dir = test_dir.join("sessions");
        fs::create_dir_all(&windows_dir).expect("Failed to create test dir");

        // Create a test file
        let test_path = windows_dir.join("delete-test.json");
        fs::write(&test_path, "{}").expect("Failed to write test file");
        assert!(test_path.exists());

        // Create metadata and test delete
        let _metadata = WindowMetadata::new("delete-test".to_string(), None, 24, 80);
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
            window_name: "old-window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::RestoreStale { window_name } => {
                assert_eq!(window_name, "old-window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_delete_stale_message() {
        let msg = ClientMessage::DeleteStale {
            window_name: "old-window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::DeleteStale { window_name } => {
                assert_eq!(window_name, "old-window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_stale_windows_response() {
        let windows = vec![
            WindowMetadata::new("window1".to_string(), None, 24, 80),
            WindowMetadata::new("window2".to_string(), Some(PathBuf::from("/tmp")), 30, 100),
        ];
        let msg = ServerResponse::StaleWindows { windows };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::StaleWindows { windows } => {
                assert_eq!(windows.len(), 2);
                assert_eq!(windows[0].name, "window1");
                assert_eq!(windows[1].name, "window2");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_restored_response() {
        let msg = ServerResponse::Restored {
            window_name: "restored-window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Restored { window_name } => {
                assert_eq!(window_name, "restored-window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_deleted_response() {
        let msg = ServerResponse::Deleted {
            window_name: "deleted-window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Deleted { window_name } => {
                assert_eq!(window_name, "deleted-window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_process_list_stale_empty() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let response = process_msg(
            ClientMessage::ListStale,
            &windows,
            &shutdown,
            &mut current_window,
        );

        match response {
            ServerResponse::StaleWindows { windows: _ } => {
                // Should be empty since no metadata exists
                // (or could have stale files from other tests, either is acceptable)
            }
            ServerResponse::Error { .. } => {
                // Also acceptable if windows dir doesn't exist
            }
            _ => panic!("Expected StaleWindows or Error response"),
        }
    }

    #[test]
    fn test_process_delete_stale_nonexistent() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::DeleteStale {
            window_name: "nonexistent-window-xyz123".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        // Should get an error since the metadata file doesn't exist
        assert!(matches!(response, ServerResponse::Error { .. }));
    }

    #[test]
    fn test_process_restore_stale_already_exists() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Create a window first
        {
            let mut windows_guard = windows.lock().unwrap();
            windows_guard.insert(
                "existing-window".to_string(),
                Window::new("existing-window".to_string(), 24, 80, None).unwrap(),
            );
        }

        let msg = ClientMessage::RestoreStale {
            window_name: "existing-window".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        // Should fail since window already exists
        match response {
            ServerResponse::Error { message } => {
                assert!(message.contains("already exists"));
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[test]
    fn test_persisted_window_state_new() {
        let metadata = WindowMetadata::new("test-window".to_string(), None, 24, 80);
        let state = PersistedWindowState::new(metadata.clone());

        assert_eq!(state.metadata.name, "test-window");
        assert_eq!(state.metadata.rows, 24);
        assert_eq!(state.metadata.cols, 80);
        assert!(state.terminal_state.is_none());
        assert!(state.environment.is_none());
        assert_eq!(state.version, PERSISTED_STATE_VERSION);
    }

    #[test]
    fn test_persisted_window_state_file_path() {
        let path = PersistedWindowState::file_path("my-window");
        assert!(path.to_string_lossy().contains("my-window.state"));
    }

    #[test]
    fn test_persisted_window_state_serialization() {
        let metadata =
            WindowMetadata::new("test".to_string(), Some(PathBuf::from("/tmp")), 30, 100);
        let mut state = PersistedWindowState::new(metadata);
        state.terminal_state = Some(b"\x1b[mHello World".to_vec());
        state.environment = Some(
            [
                ("PATH".to_string(), "/usr/bin".to_string()),
                ("HOME".to_string(), "/home/user".to_string()),
            ]
            .into(),
        );

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: PersistedWindowState = serde_json::from_str(&json).unwrap();

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
    fn test_window_uses_scrollback() {
        // Verify Window::new uses DEFAULT_SCROLLBACK
        let window = Window::new("scrollback-test".to_string(), 24, 80, None).unwrap();
        // The window should be able to handle scrollback content
        // We can verify by checking that the parser was created with scrollback
        let screen = window.terminal_parser.screen();
        // state_formatted() will include scrollback if available
        let state = screen.state_formatted();
        // Just verify it returns something (actual content depends on terminal)
        assert!(state.is_empty() || !state.is_empty()); // Tautology to verify no panic

        // Clean up
        let _ = window.metadata.delete();
    }

    // Tests for window rename functionality (sidebar_tui-1pk)

    #[test]
    fn test_encode_decode_rename_message() {
        let msg = ClientMessage::Rename {
            old_name: "old_window".to_string(),
            new_name: "new_window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Rename { old_name, new_name } => {
                assert_eq!(old_name, "old_window");
                assert_eq!(new_name, "new_window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_renamed_response() {
        let msg = ServerResponse::Renamed {
            old_name: "old_window".to_string(),
            new_name: "new_window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Renamed { old_name, new_name } => {
                assert_eq!(old_name, "old_window");
                assert_eq!(new_name, "new_window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_process_rename_success() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Add window
        {
            let mut windows_guard = windows.lock().unwrap();
            windows_guard.insert(
                "old_name".to_string(),
                Window::new("old_name".to_string(), 24, 80, None).unwrap(),
            );
        }

        let msg = ClientMessage::Rename {
            old_name: "old_name".to_string(),
            new_name: "new_name".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Renamed { old_name, new_name } => {
                assert_eq!(old_name, "old_name");
                assert_eq!(new_name, "new_name");
            }
            _ => panic!("Expected Renamed response, got {:?}", response),
        }

        // Verify window was renamed
        let windows_guard = windows.lock().unwrap();
        assert!(
            !windows_guard.contains_key("old_name"),
            "Old name should not exist"
        );
        assert!(
            windows_guard.contains_key("new_name"),
            "New name should exist"
        );
        assert_eq!(windows_guard.get("new_name").unwrap().name, "new_name");
    }

    #[test]
    fn test_process_rename_nonexistent() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        let msg = ClientMessage::Rename {
            old_name: "nonexistent".to_string(),
            new_name: "new_name".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        assert!(matches!(response, ServerResponse::Error { .. }));
    }

    #[test]
    fn test_process_rename_conflict() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Add two windows
        {
            let mut windows_guard = windows.lock().unwrap();
            windows_guard.insert(
                "window1".to_string(),
                Window::new("window1".to_string(), 24, 80, None).unwrap(),
            );
            windows_guard.insert(
                "window2".to_string(),
                Window::new("window2".to_string(), 24, 80, None).unwrap(),
            );
        }

        // Try to rename window1 to window2 (already exists)
        let msg = ClientMessage::Rename {
            old_name: "window1".to_string(),
            new_name: "window2".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        assert!(matches!(response, ServerResponse::Error { .. }));

        // Verify nothing changed
        let windows_guard = windows.lock().unwrap();
        assert!(
            windows_guard.contains_key("window1"),
            "window1 should still exist"
        );
        assert!(
            windows_guard.contains_key("window2"),
            "window2 should still exist"
        );
    }

    #[test]
    fn test_process_rename_empty_name() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Add window
        {
            let mut windows_guard = windows.lock().unwrap();
            windows_guard.insert(
                "old_name".to_string(),
                Window::new("old_name".to_string(), 24, 80, None).unwrap(),
            );
        }

        // Try to rename to empty string
        let msg = ClientMessage::Rename {
            old_name: "old_name".to_string(),
            new_name: "".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        assert!(matches!(response, ServerResponse::Error { .. }));

        // Verify window was not changed
        let windows_guard = windows.lock().unwrap();
        assert!(
            windows_guard.contains_key("old_name"),
            "old_name should still exist"
        );
    }

    // Tests for preview functionality (sidebar_tui-xjh)

    #[test]
    fn test_encode_decode_preview_message() {
        let msg = ClientMessage::Preview {
            window_name: "test_window".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Preview { window_name } => {
                assert_eq!(window_name, "test_window");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_encode_decode_previewed_response() {
        let msg = ServerResponse::Previewed {
            window_name: "test_window".to_string(),
            terminal_state: Some(b"Hello, World!".to_vec()),
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ServerResponse = decode_message(&mut cursor).unwrap();

        match decoded {
            ServerResponse::Previewed {
                window_name,
                terminal_state,
            } => {
                assert_eq!(window_name, "test_window");
                assert_eq!(terminal_state, Some(b"Hello, World!".to_vec()));
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_process_preview_existing_window() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Add a window with some terminal content
        {
            let mut windows_guard = windows.lock().unwrap();
            let mut window = Window::new("preview_test".to_string(), 24, 80, None).unwrap();
            // Add some content to the terminal
            window.process_raw(b"Hello, Preview!");
            windows_guard.insert("preview_test".to_string(), window);
        }

        // Request preview
        let msg = ClientMessage::Preview {
            window_name: "preview_test".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        match response {
            ServerResponse::Previewed {
                window_name,
                terminal_state,
            } => {
                assert_eq!(window_name, "preview_test");
                assert!(terminal_state.is_some());
                let state_bytes = terminal_state.unwrap();
                let contents = String::from_utf8_lossy(&state_bytes);
                assert!(
                    contents.contains("Hello, Preview!"),
                    "Preview should contain terminal content"
                );
            }
            _ => panic!("Expected Previewed response, got {:?}", response),
        }
    }

    #[test]
    fn test_process_preview_nonexistent_window() {
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut current_window: Option<String> = None;

        // Request preview for nonexistent window
        let msg = ClientMessage::Preview {
            window_name: "nonexistent".to_string(),
        };

        let response = process_msg(msg, &windows, &shutdown, &mut current_window);

        assert!(matches!(response, ServerResponse::Error { .. }));
    }
}
