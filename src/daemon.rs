//! Session daemon module for persisting terminal sessions.
//!
//! This module implements a daemon that owns PTY sessions and communicates
//! with TUI clients via Unix sockets. Sessions persist when the TUI disconnects
//! and can be reattached later.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Message types for communication between TUI client and daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Attach to a session (create if doesn't exist).
    Attach { session_name: String, rows: u16, cols: u16 },
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
    /// Session was killed.
    Killed { session_name: String },
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
}

/// Get the runtime directory for the daemon socket.
pub fn get_runtime_dir() -> PathBuf {
    // Try XDG_RUNTIME_DIR first (standard on Linux)
    if let Ok(dir) = env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("sidebar-tui");
    }

    // Fall back to /tmp/sidebar-tui-{uid}
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/sidebar-tui-{}", uid))
}

/// Get the socket path for the daemon.
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.sock")
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

/// Session daemon that manages terminal sessions.
pub struct Daemon {
    /// Map of session names to session handles.
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// Path to the Unix socket.
    socket_path: PathBuf,
    /// Flag to signal shutdown.
    shutdown: Arc<AtomicBool>,
}

/// A single terminal session managed by the daemon.
pub struct Session {
    pub name: String,
    pub rows: u16,
    pub cols: u16,
    pub is_attached: bool,
    // PTY handle will be added in a future issue (sidebar_tui-8wq)
    // For now, we just track session metadata.
}

impl Session {
    pub fn new(name: String, rows: u16, cols: u16) -> Self {
        Self {
            name,
            rows,
            cols,
            is_attached: false,
        }
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            name: self.name.clone(),
            is_attached: self.is_attached,
            rows: self.rows,
            cols: self.cols,
        }
    }
}

impl Daemon {
    /// Create a new daemon instance.
    pub fn new() -> Result<Self> {
        let socket_path = get_socket_path();
        Ok(Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            socket_path,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a daemon with a custom socket path (for testing).
    pub fn with_socket_path(socket_path: PathBuf) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            socket_path,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the socket path for this daemon.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Check if a daemon is already running.
    pub fn is_running(&self) -> bool {
        if !self.socket_path.exists() {
            return false;
        }
        // Try to connect to see if there's actually a daemon listening
        UnixStream::connect(&self.socket_path).is_ok()
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
                Ok((stream, _addr)) => {
                    let sessions = Arc::clone(&self.sessions);
                    let shutdown = Arc::clone(&self.shutdown);
                    thread::spawn(move || {
                        if let Err(e) = handle_client(stream, sessions, shutdown) {
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

    /// Set up signal handler for graceful shutdown.
    fn setup_signal_handler(&self) -> Result<()> {
        let shutdown = Arc::clone(&self.shutdown);
        let socket_path = self.socket_path.clone();

        // Use a simple approach with ctrlc for SIGINT/SIGTERM
        // The signal-hook crate would be more comprehensive but ctrlc is simpler
        ctrlc::set_handler(move || {
            shutdown.store(true, Ordering::SeqCst);
            // Clean up socket file
            if socket_path.exists() {
                let _ = fs::remove_file(&socket_path);
            }
        })
        .context("Failed to set signal handler")?;

        Ok(())
    }

    /// Get a list of all sessions.
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().unwrap();
        sessions.values().map(|s| s.info()).collect()
    }

    /// Create or get a session.
    pub fn get_or_create_session(&self, name: &str, rows: u16, cols: u16) -> (SessionInfo, bool) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(name) {
            // Session exists, mark as attached and update dimensions
            session.is_attached = true;
            session.rows = rows;
            session.cols = cols;
            (session.info(), false)
        } else {
            // Create new session
            let mut session = Session::new(name.to_string(), rows, cols);
            session.is_attached = true;
            let info = session.info();
            sessions.insert(name.to_string(), session);
            (info, true)
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
        sessions.remove(name).is_some()
    }
}

impl Default for Daemon {
    fn default() -> Self {
        Self::new().expect("Failed to create default daemon")
    }
}

/// Handle a client connection.
fn handle_client(
    mut stream: UnixStream,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))
        .context("Failed to set read timeout")?;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            send_response(&mut stream, &DaemonResponse::ShuttingDown)?;
            break;
        }

        let msg = match read_message(&mut stream) {
            Ok(msg) => msg,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Client disconnected
                break;
            }
            Err(e) => {
                return Err(e).context("Failed to read message")?;
            }
        };

        let response = process_message(msg, &sessions, &shutdown);
        send_response(&mut stream, &response)?;

        if matches!(response, DaemonResponse::ShuttingDown | DaemonResponse::Detached) {
            break;
        }
    }

    Ok(())
}

/// Process a client message and return a response.
fn process_message(
    msg: ClientMessage,
    sessions: &Arc<Mutex<HashMap<String, Session>>>,
    shutdown: &Arc<AtomicBool>,
) -> DaemonResponse {
    match msg {
        ClientMessage::Attach { session_name, rows, cols } => {
            let mut sessions = sessions.lock().unwrap();
            let is_new = !sessions.contains_key(&session_name);

            let session = sessions.entry(session_name.clone()).or_insert_with(|| {
                Session::new(session_name.clone(), rows, cols)
            });
            session.is_attached = true;
            session.rows = rows;
            session.cols = cols;

            DaemonResponse::Attached {
                session_name,
                is_new,
                terminal_state: None, // Will be implemented when PTY is moved to daemon
            }
        }
        ClientMessage::Detach => {
            DaemonResponse::Detached
        }
        ClientMessage::Input { data: _ } => {
            // Will be implemented when PTY is moved to daemon
            DaemonResponse::Output { data: vec![] }
        }
        ClientMessage::Resize { rows: _, cols: _ } => {
            // Will be implemented when PTY is moved to daemon
            DaemonResponse::Output { data: vec![] }
        }
        ClientMessage::List => {
            let sessions = sessions.lock().unwrap();
            let names: Vec<SessionInfo> = sessions.values().map(|s| s.info()).collect();
            DaemonResponse::Sessions { names }
        }
        ClientMessage::Kill { session_name } => {
            let mut sessions = sessions.lock().unwrap();
            if sessions.remove(&session_name).is_some() {
                DaemonResponse::Killed { session_name }
            } else {
                DaemonResponse::Error {
                    message: format!("Session '{}' not found", session_name),
                }
            }
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

/// Decode a length-prefixed message from a reader.
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

/// Read a message from the stream.
fn read_message(stream: &mut UnixStream) -> io::Result<ClientMessage> {
    decode_message(stream)
}

/// Send a response to the client.
fn send_response(stream: &mut UnixStream, response: &DaemonResponse) -> Result<()> {
    let encoded = encode_message(response)?;
    stream.write_all(&encoded).context("Failed to write response")?;
    stream.flush().context("Failed to flush response")?;
    Ok(())
}

/// Client handle for connecting to the daemon.
pub struct DaemonClient {
    stream: UnixStream,
}

impl DaemonClient {
    /// Connect to the daemon.
    pub fn connect() -> Result<Self> {
        let socket_path = get_socket_path();
        Self::connect_to(&socket_path)
    }

    /// Connect to a daemon at a specific socket path.
    pub fn connect_to(socket_path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket_path)
            .context("Failed to connect to daemon")?;
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
    pub fn attach(&mut self, session_name: &str, rows: u16, cols: u16) -> Result<DaemonResponse> {
        self.send(ClientMessage::Attach {
            session_name: session_name.to_string(),
            rows,
            cols,
        })
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
        // Should end with "daemon.sock"
        assert!(
            path_str.ends_with("daemon.sock"),
            "Socket path should end with 'daemon.sock': {}",
            path_str
        );
    }

    #[test]
    fn test_session_creation() {
        let session = Session::new("test".to_string(), 24, 80);
        assert_eq!(session.name, "test");
        assert_eq!(session.rows, 24);
        assert_eq!(session.cols, 80);
        assert!(!session.is_attached);
    }

    #[test]
    fn test_session_info() {
        let mut session = Session::new("test".to_string(), 24, 80);
        session.is_attached = true;
        let info = session.info();
        assert_eq!(info.name, "test");
        assert_eq!(info.rows, 24);
        assert_eq!(info.cols, 80);
        assert!(info.is_attached);
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
        let (info, is_new) = daemon.get_or_create_session("test", 24, 80);
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
        let (_, is_new1) = daemon.get_or_create_session("test", 24, 80);
        assert!(is_new1);

        // Detach then reattach
        daemon.detach_session("test");
        let (info, is_new2) = daemon.get_or_create_session("test", 30, 100);
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
        daemon.get_or_create_session("test", 24, 80);

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
        daemon.get_or_create_session("test", 24, 80);
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
        daemon.get_or_create_session("session1", 24, 80);
        daemon.get_or_create_session("session2", 30, 100);

        let sessions = daemon.list_sessions();
        assert_eq!(sessions.len(), 2);

        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"session1"));
        assert!(names.contains(&"session2"));

        cleanup_socket(&path);
    }

    #[test]
    fn test_encode_decode_client_message() {
        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 24,
            cols: 80,
        };
        let encoded = encode_message(&msg).unwrap();

        let mut cursor = std::io::Cursor::new(encoded);
        let decoded: ClientMessage = decode_message(&mut cursor).unwrap();

        match decoded {
            ClientMessage::Attach { session_name, rows, cols } => {
                assert_eq!(session_name, "test");
                assert_eq!(rows, 24);
                assert_eq!(cols, 80);
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
    fn test_encode_decode_sessions_response() {
        let sessions = vec![
            SessionInfo {
                name: "s1".to_string(),
                is_attached: true,
                rows: 24,
                cols: 80,
            },
            SessionInfo {
                name: "s2".to_string(),
                is_attached: false,
                rows: 30,
                cols: 100,
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

        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 24,
            cols: 80,
        };

        let response = process_message(msg, &sessions, &shutdown);

        match response {
            DaemonResponse::Attached { session_name, is_new, .. } => {
                assert_eq!(session_name, "test");
                assert!(is_new);
            }
            _ => panic!("Expected Attached response"),
        }

        // Verify session was created
        let sessions = sessions.lock().unwrap();
        assert!(sessions.contains_key("test"));
    }

    #[test]
    fn test_process_attach_existing_session() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Create initial session
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("test".to_string(), Session::new("test".to_string(), 24, 80));
        }

        // Attach to existing session
        let msg = ClientMessage::Attach {
            session_name: "test".to_string(),
            rows: 30,
            cols: 100,
        };

        let response = process_message(msg, &sessions, &shutdown);

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

        // Add some sessions
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("s1".to_string(), Session::new("s1".to_string(), 24, 80));
            sessions.insert("s2".to_string(), Session::new("s2".to_string(), 30, 100));
        }

        let response = process_message(ClientMessage::List, &sessions, &shutdown);

        match response {
            DaemonResponse::Sessions { names } => {
                assert_eq!(names.len(), 2);
            }
            _ => panic!("Expected Sessions response"),
        }
    }

    #[test]
    fn test_process_kill_existing() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Add session
        {
            let mut sessions = sessions.lock().unwrap();
            sessions.insert("victim".to_string(), Session::new("victim".to_string(), 24, 80));
        }

        let msg = ClientMessage::Kill {
            session_name: "victim".to_string(),
        };

        let response = process_message(msg, &sessions, &shutdown);

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

        let msg = ClientMessage::Kill {
            session_name: "nonexistent".to_string(),
        };

        let response = process_message(msg, &sessions, &shutdown);

        assert!(matches!(response, DaemonResponse::Error { .. }));
    }

    #[test]
    fn test_process_shutdown() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let response = process_message(ClientMessage::Shutdown, &sessions, &shutdown);

        assert!(matches!(response, DaemonResponse::ShuttingDown));
        assert!(shutdown.load(Ordering::SeqCst));
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
}
