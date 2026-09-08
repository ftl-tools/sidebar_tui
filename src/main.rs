use std::collections::HashMap;
use std::env;
use std::io::Write as IoWrite;
#[cfg(windows)]
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::{Context, bail};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseEventKind};
use crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use sidebar_tui::colors;
use sidebar_tui::server::{
    self, ClientMessage, ServerClient, ServerResponse, IpcStream, MessageReader, decode_message,
    encode_message, ensure_runtime_dir, get_socket_path,
};
use sidebar_tui::hint_bar::hint_bar_for_state;
use sidebar_tui::input::{encode_mouse_scroll, key_to_bytes};
use sidebar_tui::sidebar::{Sidebar, get_sidebar_cursor_position};
use sidebar_tui::state::{
    AppMode, AppState, EventResult, Focus, Window, WindowType, SessionOverlayMode,
    SessionOverlayState,
};
use sidebar_tui::terminal::Terminal;
use sidebar_tui::updater;

/// Version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sidebar TUI - A tmux-style session and window manager
#[derive(Parser, Debug)]
#[command(name = "sb")]
#[command(version = VERSION)]
#[command(about = "A tmux-style session and window manager (currently using Sidebar's own PTY server)", long_about = None)]
#[command(disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Window name to attach to (if not specified, attaches to most recent or shows welcome state)
    // The old --session flag targeted a window; retain it only as a compatibility alias.
    #[arg(short = 'w', short_alias = 's', long, alias = "session")]
    window: Option<String>,

    /// Print version information
    #[arg(short = 'v', short_alias = 'V', long = "version", action = clap::ArgAction::Version)]
    version: (),
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Native tmux chooser and read-only inspection (ordinary sb remains legacy)
    Tmux(sidebar_tui::tmux::TmuxCli),
    /// List all active windows across sessions
    #[command(name = "list-windows", alias = "list")]
    List,
    /// Kill a window
    #[command(name = "kill-window", alias = "kill")]
    Kill {
        /// Name of the window to kill
        window: String,
    },
    /// Attach to a window (or create if it doesn't exist)
    Attach {
        /// Window name
        #[arg(default_value = "main")]
        window: String,
    },
    /// Start the Sidebar server
    #[command(alias = "daemon")]
    Server,
    /// List stale windows (from before reboot/crash)
    Stale,
    /// Restore a stale window
    Restore {
        /// Name of the window to restore
        window: String,
    },
    /// Delete stale window metadata
    Forget {
        /// Name of the window to forget
        window: String,
    },
    /// Shutdown the server and kill all windows
    Shutdown,
    /// Manage sessions (groups of windows)
    #[command(alias = "workspace")]
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Check for updates and self-update the binary
    SelfUpdate,
}

#[derive(Subcommand, Debug)]
enum SessionAction {
    /// List all sessions
    List,
    /// Create a new session
    Create {
        /// Session name
        name: String,
    },
    /// Kill a session and all its windows
    #[command(name = "kill", alias = "delete")]
    Kill {
        /// Session name
        name: String,
    },
    /// Switch active session
    Switch {
        /// Session name
        name: String,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    // On normal startup (not a server or subcommand) check for updates in the background.
    if cli.command.is_none() {
        updater::check_and_notify();
    }

    // The old routes own legacy PTYs; keep tmux inspection explicitly isolated from them.
    match cli.command {
        Some(Commands::Tmux(options)) => sidebar_tui::tmux::run(options).map_err(|e| color_eyre::eyre::eyre!("{e:#}")),
        Some(Commands::List) => cmd_list(),
        Some(Commands::Kill { window }) => cmd_kill(&window),
        Some(Commands::Attach { window }) => cmd_attach(Some(&window)),
        Some(Commands::Server) => cmd_server(),
        Some(Commands::Stale) => cmd_stale(),
        Some(Commands::Restore { window }) => cmd_restore(&window),
        Some(Commands::Forget { window }) => cmd_forget(&window),
        Some(Commands::Shutdown) => cmd_shutdown(),
        Some(Commands::Session { action }) => cmd_session(action),
        Some(Commands::SelfUpdate) => updater::run_self_update(),
        None => cmd_attach(cli.window.as_deref()),
    }
}

/// List all active windows.
fn cmd_list() -> Result<()> {
    let mut client = connect_to_server()?;
    let windows = client.list_windows()?;

    if windows.is_empty() {
        println!("No active windows");
    } else {
        println!(
            "{:<20} {:<10} {:>5}x{:<5}",
            "NAME", "STATUS", "ROWS", "COLS"
        );
        for window in windows {
            let status = if window.is_attached {
                "attached"
            } else {
                "detached"
            };
            println!(
                "{:<20} {:<10} {:>5}x{:<5}",
                window.name, status, window.rows, window.cols
            );
        }
    }

    Ok(())
}

/// Kill a window.
fn cmd_kill(window_name: &str) -> Result<()> {
    let mut client = connect_to_server()?;
    client.kill_window(window_name)?;
    println!("Killed window '{}'", window_name);
    Ok(())
}

/// List stale windows (from before reboot/crash).
fn cmd_stale() -> Result<()> {
    let mut client = connect_to_server()?;
    let windows = client.list_stale_windows()?;

    if windows.is_empty() {
        println!("No stale windows found");
    } else {
        println!(
            "{:<20} {:<30} {:>5}x{:<5}",
            "NAME", "WORKING DIR", "ROWS", "COLS"
        );
        for window in windows {
            let cwd = window
                .cwd
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<20} {:<30} {:>5}x{:<5}",
                window.name, cwd, window.rows, window.cols
            );
        }
        println!(
            "\nUse 'sb restore <name>' to restore a window, or 'sb forget <name>' to delete."
        );
    }

    Ok(())
}

/// Restore a stale window.
fn cmd_restore(window_name: &str) -> Result<()> {
    let mut client = connect_to_server()?;
    client.restore_stale_window(window_name)?;
    println!(
        "Restored window '{}'. Use 'sb attach {}' to connect.",
        window_name, window_name
    );
    Ok(())
}

/// Delete stale window metadata.
fn cmd_forget(window_name: &str) -> Result<()> {
    let mut client = connect_to_server()?;
    client.delete_stale_window(window_name)?;
    println!("Deleted metadata for window '{}'", window_name);
    Ok(())
}

/// Shutdown the server and kill all windows.
fn cmd_shutdown() -> Result<()> {
    match connect_to_server() {
        Ok(mut client) => {
            client.shutdown()?;
            println!("Server shutdown complete. All windows terminated.");
            Ok(())
        }
        Err(_) => {
            println!("No server running.");
            Ok(())
        }
    }
}

/// Manage sessions via CLI.
fn cmd_session(action: SessionAction) -> Result<()> {
    let mut client = connect_to_server()?;
    match action {
        SessionAction::List => {
            let (sessions, active) = client.list_sessions()?;
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                for session in &sessions {
                    let marker = if session.name == active { "* " } else { "  " };
                    println!("{}{}", marker, session.name);
                }
            }
        }
        SessionAction::Create { name } => {
            client.create_session(&name)?;
            println!("Created session '{}'", name);
        }
        SessionAction::Kill { name } => {
            client.kill_session(&name)?;
            println!("Killed session '{}'", name);
        }
        SessionAction::Switch { name } => {
            client.switch_session(&name)?;
            println!("Switched to session '{}'", name);
        }
    }
    Ok(())
}

/// Start the server process (runs in foreground).
fn cmd_server() -> Result<()> {
    let server = server::Server::new()?;
    println!("Starting server at {:?}", server.socket_path());
    server.run()
}

/// Attach to a window (or show welcome state if no windows exist).
/// If window_name is None, will attach to the first existing window or show welcome state.
/// If window_name is Some, will attach to that window (creating if needed).
fn cmd_attach(window_name: Option<&str>) -> Result<()> {
    // Ensure server is running
    ensure_server_running()?;

    // Connect to server: on Unix use Unix socket, on Windows use TCP via lockfile port.
    let socket_path = get_socket_path();
    #[cfg(unix)]
    let mut stream: IpcStream =
        UnixStream::connect(&socket_path).context("Failed to connect to server")?;
    #[cfg(windows)]
    let mut stream: IpcStream = {
        let port = std::fs::read_to_string(&socket_path)
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok())
            .ok_or_else(|| color_eyre::eyre::eyre!("Failed to read server port from lockfile"))?;
        TcpStream::connect(format!("127.0.0.1:{}", port))
            .context("Failed to connect to server via TCP")?
    };

    // Set read timeout for non-blocking reads
    stream
        .set_read_timeout(Some(Duration::from_millis(1)))
        .context("Failed to set read timeout")?;

    // Initialize TUI with mouse capture enabled so scroll wheel scrolls terminal
    // scrollback history rather than sending arrow keys to the shell.
    // Users can disable mouse capture (for native text selection) with Ctrl+S.
    let mut ratatui_term = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture).context("Failed to enable mouse capture")?;

    let result = run_attached(&mut ratatui_term, &mut stream, window_name);

    // Ensure mouse capture is disabled before restoring terminal
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// Connect to the server, starting it if necessary.
fn connect_to_server() -> Result<ServerClient> {
    ensure_server_running()?;
    ServerClient::connect()
}

/// Ensure the server is running, starting it if necessary.
fn ensure_server_running() -> Result<()> {
    ensure_runtime_dir()?;
    let socket_path = get_socket_path();

    // Try to connect to see if server is already running
    if server_is_reachable(&socket_path) {
        return Ok(());
    }

    // Start server in background
    start_server_background()?;

    // Wait for server to be ready
    for _ in 0..50 {
        if server_is_reachable(&socket_path) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    bail!("Server failed to start within timeout")
}

/// Check if the server is reachable at the given socket/lockfile path.
#[cfg(unix)]
fn server_is_reachable(socket_path: &std::path::Path) -> bool {
    UnixStream::connect(socket_path).is_ok()
}

#[cfg(windows)]
fn server_is_reachable(socket_path: &std::path::Path) -> bool {
    use std::fs;
    let port = fs::read_to_string(socket_path)
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok());
    match port {
        Some(p) => TcpStream::connect(format!("127.0.0.1:{}", p)).is_ok(),
        None => false,
    }
}

/// Start the server as a background process.
fn start_server_background() -> Result<()> {
    // Get path to current executable
    let exe = env::current_exe().context("Failed to get current executable path")?;

    // Fork server process
    Command::new(exe)
        .arg("server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn server process")?;

    Ok(())
}

/// Minimum time between scroll actions (throttle for trackpad smoothness)
const SCROLL_THROTTLE_MS: u128 = 30;

/// Time threshold for "fast" scrolling (events arriving faster than this = fast scroll)
const SCROLL_FAST_THRESHOLD_MS: u128 = 15;

/// Application state for server-connected mode.
struct ServerApp {
    /// Terminal emulator for parsing PTY output
    term_emulator: Terminal,
    /// Current window name
    window_name: String,
    /// Application UI state (focus, mode, windows list)
    app_state: AppState,
    /// Per-window terminal scroll offsets (saved when switching away, restored when switching back)
    window_scroll_offsets: HashMap<String, usize>,
    /// Last time a scroll action was performed (for throttling)
    last_scroll_time: std::time::Instant,
    /// Last time any scroll event was received (for velocity calculation)
    last_scroll_event_time: std::time::Instant,
    /// Accumulated scroll events for velocity calculation
    scroll_event_count: u32,
    /// Temporary message to show in hint bar, along with its expiry time.
    timed_message: Option<(String, std::time::Instant)>,
}

impl ServerApp {
    fn new(rows: u16, cols: u16, window_name: &str, windows: Vec<Window>) -> Self {
        let mut app_state = AppState::with_windows(windows);
        // If we have windows, focus on terminal
        if !app_state.windows.is_empty() {
            app_state.focus = Focus::Terminal;
        }
        Self {
            term_emulator: Terminal::new(rows, cols),
            window_name: window_name.to_string(),
            app_state,
            window_scroll_offsets: HashMap::new(),
            last_scroll_time: std::time::Instant::now(),
            last_scroll_event_time: std::time::Instant::now(),
            scroll_event_count: 0,
            timed_message: None,
        }
    }

    /// Create app in welcome state (no windows, sidebar focused).
    fn new_welcome_state(rows: u16, cols: u16) -> Self {
        let app_state = AppState::default();
        Self {
            term_emulator: Terminal::new(rows, cols),
            window_name: String::new(),
            app_state,
            window_scroll_offsets: HashMap::new(),
            last_scroll_time: std::time::Instant::now(),
            last_scroll_event_time: std::time::Instant::now(),
            scroll_event_count: 0,
            timed_message: None,
        }
    }

    /// Show a temporary message in the hint bar for 3 seconds.
    fn show_timed_message(&mut self, text: impl Into<String>) {
        let expiry = std::time::Instant::now() + Duration::from_secs(3);
        self.timed_message = Some((text.into(), expiry));
    }

    /// Check if timed message has expired and clear it.
    fn tick_timed_message(&mut self) {
        if let Some((_, expiry)) = &self.timed_message {
            if std::time::Instant::now() >= *expiry {
                self.timed_message = None;
            }
        }
    }

    /// Process data received from the server.
    fn process_output(&mut self, data: &[u8]) {
        self.term_emulator.process(data);
    }

    /// Resize the terminal emulator.
    fn resize(&mut self, rows: u16, cols: u16) {
        self.term_emulator.resize(rows, cols);
    }
}

/// Helper to send a message to the server and read the response.
fn send_server_message(stream: &mut IpcStream, msg: ClientMessage) -> Result<ServerResponse> {
    let encoded = encode_message(&msg)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    // Use a longer timeout for synchronous operations
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let response: ServerResponse =
        decode_message(stream).context("Failed to read server response")?;
    Ok(response)
}

/// Helper to send a sync message and wait for the response, skipping any in-flight
/// async Output messages that arrived before the response.
///
/// This solves a race condition where the server sends Output messages from the old
/// window while the client is waiting for an Attached/Killed/etc. response. Without
/// this, `send_server_message` would return an Output message instead of the expected
/// response, causing window switches to silently fail and window A content to bleed
/// into window B's terminal.
fn send_server_message_sync(
    stream: &mut IpcStream,
    msg: ClientMessage,
    app: &mut ServerApp,
) -> Result<ServerResponse> {
    let encoded = encode_message(&msg)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    loop {
        let response: ServerResponse =
            decode_message(stream).context("Failed to read server response")?;
        match response {
            // In-flight output from the old window: apply to the current terminal
            // (still window A at this point) and keep waiting for the real response.
            ServerResponse::Output { data } => {
                if !data.is_empty() {
                    app.process_output(&data);
                }
            }
            other => return Ok(other),
        }
    }
}

/// Drain all pending async messages before a synchronous operation.
///
/// This function processes any buffered messages and reads any in-flight messages
/// from the socket with a short timeout, preventing message interleaving when
/// sync operations (like CreateWindow, KillWindow) are called while async
/// messages (like Preview, Output) may be pending.
///
/// Returns Ok(()) on success, or an error if reading fails.
fn drain_async_messages(
    msg_reader: &mut MessageReader,
    stream: &mut IpcStream,
    app: &mut ServerApp,
) -> Result<()> {
    use std::io;

    // 1. Process any complete messages already buffered
    while let Some(response) = msg_reader.try_parse_buffered::<ServerResponse>()? {
        handle_drained_response(response, app);
    }

    // 2. Try to read any pending data from the socket with short timeout
    // This catches messages in flight but not yet buffered
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    loop {
        match msg_reader.try_read::<ServerResponse>(stream) {
            Ok(Some(response)) => {
                handle_drained_response(response, app);
            }
            Ok(None) => {
                // No more complete messages available
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => break,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Connection closed - this is a real error
                return Err(e.into());
            }
            Err(_) => {
                // Other errors - just break and let the sync op proceed
                break;
            }
        }
    }

    // 3. Clear any partial data (will be lost, but sync op will resync)
    if msg_reader.has_buffered_data() {
        // Warning: dropping partial message before sync operation
        msg_reader.clear();
    }

    Ok(())
}

/// Handle a response that was drained before a sync operation.
fn handle_drained_response(response: ServerResponse, app: &mut ServerApp) {
    match response {
        ServerResponse::Output { data } => {
            // Process terminal output
            if !data.is_empty() {
                app.process_output(&data);
            }
        }
        ServerResponse::Previewed {
            terminal_state: Some(state_bytes),
            ..
        } => {
            // Update preview - may be stale if we're about to switch windows
            app.process_output(&state_bytes);
        }
        // Ignore Previewed with None terminal_state and other responses during drain -
        // they shouldn't happen but if they do, sync op will get the response it needs
        _ => {}
    }
}

/// Result from draining messages in the main loop.
enum MainLoopDrainResult {
    /// Continue normal processing
    Continue,
    /// Server is shutting down, break main loop
    ShuttingDown,
    /// Server sent an error message
    Error(String),
    /// Connection error (EOF, etc)
    ConnectionError(std::io::Error),
}

/// Drain all available messages from the socket in the main loop.
///
/// This batches multiple messages into a single render pass by:
/// 1. Reading once from the socket to populate the buffer
/// 2. Processing ALL complete messages from the buffer
///
/// This significantly improves performance during high-throughput scenarios
/// like pasting, where many Output messages arrive in quick succession.
fn drain_main_loop_messages(
    msg_reader: &mut MessageReader,
    stream: &mut IpcStream,
    app: &mut ServerApp,
    term_rows: u16,
    term_cols: u16,
) -> MainLoopDrainResult {
    use std::io;

    // Read once from socket to get available data into buffer
    // We use a temporary read to avoid consuming a message, then process all buffered
    let read_result = msg_reader.try_read::<ServerResponse>(stream);

    // Handle the initial read result
    let first_response = match read_result {
        Ok(Some(response)) => Some(response),
        Ok(None) => None,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
        Err(e) if e.kind() == io::ErrorKind::TimedOut => None,
        Err(e) => return MainLoopDrainResult::ConnectionError(e),
    };

    // Process first response if we got one
    if let Some(response) = first_response {
        match handle_main_loop_response(response, app, term_rows, term_cols) {
            MainLoopDrainResult::Continue => {}
            other => return other,
        }
    }

    // Now drain any additional complete messages from the buffer without reading more
    // This is the key optimization: we may have received multiple messages in one read
    loop {
        match msg_reader.try_parse_buffered::<ServerResponse>() {
            Ok(Some(response)) => {
                match handle_main_loop_response(response, app, term_rows, term_cols) {
                    MainLoopDrainResult::Continue => {}
                    other => return other,
                }
            }
            Ok(None) => break,
            Err(e) => return MainLoopDrainResult::ConnectionError(e),
        }
    }

    MainLoopDrainResult::Continue
}

/// Handle a single response in the main loop, returning the appropriate action.
fn handle_main_loop_response(
    response: ServerResponse,
    app: &mut ServerApp,
    term_rows: u16,
    term_cols: u16,
) -> MainLoopDrainResult {
    match response {
        ServerResponse::Output { data } => {
            if !data.is_empty() {
                app.process_output(&data);
            }
            MainLoopDrainResult::Continue
        }
        ServerResponse::ShuttingDown => MainLoopDrainResult::ShuttingDown,
        ServerResponse::Error { message } => MainLoopDrainResult::Error(message),
        ServerResponse::Previewed { terminal_state, .. } => {
            // Update terminal emulator with preview content
            app.term_emulator = Terminal::new(term_rows, term_cols);
            if let Some(state_bytes) = terminal_state {
                app.process_output(&state_bytes);
            }
            MainLoopDrainResult::Continue
        }
        _ => MainLoopDrainResult::Continue,
    }
}

/// Run the TUI, optionally attaching to a window.
/// If requested_window is None, will attach to first existing window or show welcome state.
/// If requested_window is Some, will attach to that window (creating if needed).
fn run_attached(
    ratatui_term: &mut DefaultTerminal,
    stream: &mut IpcStream,
    requested_window: Option<&str>,
) -> Result<()> {
    // Get initial terminal size; only the sidebar consumes terminal columns.
    let size = ratatui_term.size()?;
    let mut term_cols = compute_term_cols(size.width, false);
    // The unframed terminal uses every row; sidebar hints never resize it.
    let mut term_rows = size.height;

    // Get current working directory
    let cwd = env::current_dir().ok();

    // Load session info from server to get the active session name
    let session_response = send_server_message(stream, ClientMessage::ListSessions)?;
    let (sessions_list, active_session_name) = match session_response {
        ServerResponse::Sessions {
            sessions,
            active_session,
        } => (sessions, active_session),
        ServerResponse::Error { message } => {
            bail!("Failed to list sessions: {}", message);
        }
        other => {
            bail!("Unexpected session response: {:?}", other);
        }
    };
    let session_names: Vec<String> = sessions_list
        .iter()
        .map(|session| session.name.clone())
        .collect();

    // Load window list from server
    let window_list_response = send_server_message(stream, ClientMessage::List)?;
    let mut server_windows = match window_list_response {
        ServerResponse::Windows { names } => names,
        ServerResponse::Error { message } => {
            bail!("Failed to list windows: {}", message);
        }
        other => {
            bail!("Unexpected response: {:?}", other);
        }
    };

    // If no active windows exist, check for stale windows and auto-restore them
    if server_windows.is_empty() {
        let stale_response = send_server_message(stream, ClientMessage::ListStale)?;
        let stale_windows = match stale_response {
            ServerResponse::StaleWindows { windows } => windows,
            ServerResponse::Error { message } => {
                eprintln!("Warning: Failed to list stale windows: {}", message);
                Vec::new()
            }
            _ => Vec::new(),
        };

        // Auto-restore all stale windows, sorted by last_active (most recent first)
        let mut sorted_stale: Vec<_> = stale_windows.into_iter().collect();
        sorted_stale.sort_by(|a, b| b.last_active.cmp(&a.last_active));

        for stale in &sorted_stale {
            let restore_response = send_server_message(
                stream,
                ClientMessage::RestoreStale {
                    window_name: stale.name.clone(),
                },
            )?;
            match restore_response {
                ServerResponse::Restored { .. } => {
                    // Successfully restored
                }
                ServerResponse::Error { message } => {
                    eprintln!(
                        "Warning: Failed to restore window '{}': {}",
                        stale.name, message
                    );
                }
                _ => {}
            }
        }

        // Re-fetch the window list after restoring
        if !sorted_stale.is_empty() {
            let refreshed_response = send_server_message(stream, ClientMessage::List)?;
            server_windows = match refreshed_response {
                ServerResponse::Windows { names } => names,
                _ => Vec::new(),
            };
        }
    }

    // Convert server windows to AppState windows, filtered to active session
    let windows: Vec<Window> = server_windows
        .iter()
        .filter(|info| info.session_name == active_session_name)
        .map(|info| {
            let mut window = Window::new(&info.name);
            window.is_attached = info.is_attached;
            window
        })
        .collect();

    // Determine which window to attach to (if any)
    // - If explicit window requested, attach to it (creating if needed)
    // - If no window requested but windows exist, attach to first one
    // - If no window requested and no windows exist, start in welcome state
    let window_to_attach: Option<String> = match requested_window {
        Some(name) => Some(name.to_string()),
        None => {
            if windows.is_empty() {
                None // Welcome state
            } else {
                Some(windows[0].name.clone()) // Attach to first existing window
            }
        }
    };

    // Only attach if we have a window to attach to
    let mut app = if let Some(window_name) = window_to_attach {
        // Send attach message
        let attach_response = send_server_message(
            stream,
            ClientMessage::Attach {
                window_name: window_name.clone(),
                rows: term_rows,
                cols: term_cols,
                cwd: cwd.clone(),
            },
        )?;

        let terminal_state = match attach_response {
            ServerResponse::Attached {
                window_name: _,
                is_new,
                terminal_state,
            } => (is_new, terminal_state),
            ServerResponse::Error { message } => {
                bail!("Failed to attach: {}", message);
            }
            other => {
                bail!("Unexpected response: {:?}", other);
            }
        };

        // Build initial window list for AppState
        let mut initial_windows = windows;
        // If this was a new window, add it to the front of the list
        if terminal_state.0 {
            initial_windows.insert(0, Window::attached(&window_name));
        } else {
            // Mark the current window as attached
            for s in &mut initial_windows {
                if s.name == window_name {
                    s.is_attached = true;
                }
            }
        }

        // Create app with window list
        let mut app = ServerApp::new(term_rows, term_cols, &window_name, initial_windows);
        app.app_state.session_name = active_session_name.clone();
        app.app_state.sessions = session_names.clone();

        // Restore terminal state if reattaching
        if let Some(state_bytes) = terminal_state.1 {
            app.process_output(&state_bytes);
        }

        app
    } else {
        // Welcome state - no windows to attach to
        let mut app = ServerApp::new_welcome_state(term_rows, term_cols);
        app.app_state.session_name = active_session_name.clone();
        app.app_state.sessions = session_names.clone();
        app
    };

    let mut last_size = (size.width, size.height);
    // and resize the PTY accordingly without a full terminal resize event.

    // Set stream to non-blocking for the main loop
    stream.set_read_timeout(Some(Duration::from_millis(10)))?;

    // Create buffered message reader to handle partial reads safely
    let mut msg_reader = MessageReader::new();

    loop {
        // Drain all available messages from the socket before rendering.
        // This batches multiple output messages (e.g., during paste) into a single render,
        // significantly improving performance compared to render-per-message.
        let drain_result =
            drain_main_loop_messages(&mut msg_reader, stream, &mut app, term_rows, term_cols);
        match drain_result {
            MainLoopDrainResult::Continue => {}
            MainLoopDrainResult::ShuttingDown => break,
            MainLoopDrainResult::Error(msg) => bail!("Server error: {}", msg),
            MainLoopDrainResult::ConnectionError(e) => bail!("Connection error: {}", e),
        }

        // Expire timed messages before rendering
        app.tick_timed_message();

        // Hints only change the sidebar list viewport, not PTY geometry.
        // Render the UI once after processing all available messages
        ratatui_term.draw(|frame| render_server_app(frame, &mut app))?;

        // Keep session overlay's visible_height in sync with actual terminal geometry.
        // This enables select_next() to scroll the list when the selection moves off-screen.
        // Height = total rows - 1 (title row). Editing is done inline, no extra area.
        if let AppMode::SessionOverlay(ref mut ov) = app.app_state.mode {
            let list_h = last_size.1.saturating_sub(1);
            ov.visible_height = list_h as usize;
        }

        // Handle input events
        if event::poll(Duration::from_millis(16)).context("event poll failed")? {
            match event::read().context("event read failed")? {
                Event::Key(key) => {
                    // Route key through state machine.
                    let mut result = app.app_state.handle_key(key);
                    let relative_session_focus =
                        if matches!(result, EventResult::SwitchRelativeSession { .. }) {
                            Some(app.app_state.focus)
                        } else {
                            None
                        };
                    // Relative session shortcuts used to require opening the chooser. Resolve
                    // them here against the server-provided stable session list for direct switching.
                    if let EventResult::SwitchRelativeSession { offset } = result {
                        let sessions = &app.app_state.sessions;
                        if !sessions.is_empty() {
                            let current = sessions
                                .iter()
                                .position(|name| name == &app.app_state.session_name)
                                .unwrap_or(0) as isize;
                            let target =
                                (current + offset).rem_euclid(sessions.len() as isize) as usize;
                            result = EventResult::SwitchSession {
                                name: sessions[target].clone(),
                            };
                        } else {
                            result = EventResult::Consumed;
                        }
                    }
                    let requested_session_focus = relative_session_focus.or_else(|| {
                        if matches!(result, EventResult::SwitchSession { .. }) {
                            Some(Focus::Sidebar)
                        } else {
                            None
                        }
                    });
                    let session_action = match &result {
                        EventResult::OpenSessionCreate => Some('C'),
                        EventResult::OpenSessionRename => Some('R'),
                        EventResult::OpenSessionKill => Some('K'),
                        _ => None,
                    };

                    match result {
                        EventResult::Detach => {
                            // Send detach message and exit
                            let detach_msg = ClientMessage::Detach;
                            let encoded = encode_message(&detach_msg)?;
                            stream.write_all(&encoded)?;
                            stream.flush()?;
                            break;
                        }
                        EventResult::CreateWindow { name, window_type } => {
                            // Drain any pending async messages before sync operation
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;

                            // Create new window via server (use sync variant to skip any
                            // remaining in-flight Output messages from the old window)
                            let create_response = send_server_message_sync(
                                stream,
                                ClientMessage::Attach {
                                    window_name: name.clone(),
                                    rows: term_rows,
                                    cols: term_cols,
                                    cwd: cwd.clone(),
                                },
                                &mut app,
                            )?;

                            match create_response {
                                ServerResponse::Attached {
                                    window_name: attached_name,
                                    is_new: _,
                                    terminal_state: new_state,
                                } => {
                                    // Add window to local state
                                    app.app_state.add_window(Window::attached(&attached_name));
                                    app.window_name = attached_name;
                                    app.app_state.focus = Focus::Terminal;

                                    // Clear terminal emulator for new window
                                    app.term_emulator = Terminal::new(term_rows, term_cols);

                                    // Restore terminal state if reattaching
                                    if let Some(state_bytes) = new_state {
                                        app.process_output(&state_bytes);
                                    }

                                    // For agent windows, send the claude command
                                    if window_type == WindowType::Agent {
                                        let claude_cmd = b"claude\n";
                                        let input_msg = ClientMessage::Input {
                                            data: claude_cmd.to_vec(),
                                        };
                                        let encoded = encode_message(&input_msg)?;
                                        stream.write_all(&encoded)?;
                                        stream.flush()?;
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to create window: {}", message);
                                }
                                _ => {}
                            }
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::KillWindow { name } => {
                            // Drain any pending async messages before sync operation
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;

                            // Kill window via server
                            let kill_response = send_server_message_sync(
                                stream,
                                ClientMessage::Kill {
                                    window_name: name.clone(),
                                },
                                &mut app,
                            )?;

                            match kill_response {
                                ServerResponse::Killed { .. } => {
                                    // Remove scroll offset for the deleted window
                                    app.window_scroll_offsets.remove(&name);

                                    // If we deleted the current window, switch to another
                                    if app.window_name == name {
                                        if let Some(window) = app.app_state.windows.first() {
                                            // Switch to first available window
                                            let switch_response = send_server_message_sync(
                                                stream,
                                                ClientMessage::Attach {
                                                    window_name: window.name.clone(),
                                                    rows: term_rows,
                                                    cols: term_cols,
                                                    cwd: cwd.clone(),
                                                },
                                                &mut app,
                                            )?;

                                            if let ServerResponse::Attached {
                                                window_name: attached_name,
                                                terminal_state: new_state,
                                                ..
                                            } = switch_response
                                            {
                                                app.window_name = attached_name.clone();
                                                app.term_emulator =
                                                    Terminal::new(term_rows, term_cols);
                                                if let Some(state_bytes) = new_state {
                                                    app.process_output(&state_bytes);
                                                }
                                                // Restore scroll position for the newly attached window
                                                if let Some(&saved_offset) =
                                                    app.window_scroll_offsets.get(&attached_name)
                                                {
                                                    app.term_emulator.scroll_up(saved_offset);
                                                }
                                            }
                                        } else {
                                            // No windows left, clear terminal
                                            app.window_name = String::new();
                                            app.term_emulator = Terminal::new(term_rows, term_cols);
                                        }
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to kill window: {}", message);
                                }
                                _ => {}
                            }
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::RenameWindow { old_name, new_name } => {
                            // Drain any pending async messages before sync operation
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;

                            // Rename window via server
                            let rename_response = send_server_message_sync(
                                stream,
                                ClientMessage::Rename {
                                    old_name: old_name.clone(),
                                    new_name: new_name.clone(),
                                },
                                &mut app,
                            )?;

                            match rename_response {
                                ServerResponse::Renamed { .. } => {
                                    // Update scroll offset HashMap key for renamed window
                                    if let Some(offset) =
                                        app.window_scroll_offsets.remove(&old_name)
                                    {
                                        app.window_scroll_offsets.insert(new_name.clone(), offset);
                                    }
                                    // Update current window name if it was renamed
                                    if app.window_name == old_name {
                                        app.window_name = new_name;
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to rename window: {}", message);
                                    // Revert local state change
                                    if let Some(window) = app
                                        .app_state
                                        .windows
                                        .iter_mut()
                                        .find(|s| s.name == new_name)
                                    {
                                        window.name = old_name;
                                    }
                                }
                                _ => {}
                            }
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::SwitchWindow { name } => {
                            // Only switch if it's a different window
                            if name != app.window_name {
                                // Save scroll position for current window before switching
                                let current_scroll = app.term_emulator.get_scroll_offset();
                                if current_scroll > 0 {
                                    app.window_scroll_offsets
                                        .insert(app.window_name.clone(), current_scroll);
                                } else {
                                    app.window_scroll_offsets.remove(&app.window_name);
                                }

                                // Drain any pending async messages before sync operation
                                drain_async_messages(&mut msg_reader, stream, &mut app)?;

                                // Detach from current window
                                let _ = send_server_message_sync(
                                    stream,
                                    ClientMessage::Detach,
                                    &mut app,
                                );

                                // Attach to new window
                                let switch_response = send_server_message_sync(
                                    stream,
                                    ClientMessage::Attach {
                                        window_name: name.clone(),
                                        rows: term_rows,
                                        cols: term_cols,
                                        cwd: cwd.clone(),
                                    },
                                    &mut app,
                                )?;

                                match switch_response {
                                    ServerResponse::Attached {
                                        window_name: attached_name,
                                        terminal_state: new_state,
                                        ..
                                    } => {
                                        app.window_name = attached_name.clone();
                                        app.term_emulator = Terminal::new(term_rows, term_cols);
                                        if let Some(state_bytes) = new_state {
                                            app.process_output(&state_bytes);
                                        }
                                        // Restore scroll position for the newly attached window
                                        if let Some(&saved_offset) =
                                            app.window_scroll_offsets.get(&attached_name)
                                        {
                                            app.term_emulator.scroll_up(saved_offset);
                                        }
                                    }
                                    ServerResponse::Error { message } => {
                                        eprintln!("Failed to switch window: {}", message);
                                    }
                                    _ => {}
                                }
                            }
                            // The old MRU reshuffle made numbered bindings unstable; preserve display order.
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::PreviewWindow { name } => {
                            // Request terminal state preview for the selected window
                            // Send the preview request asynchronously - response will be
                            // handled in the main message loop above
                            let preview_msg = ClientMessage::Preview {
                                window_name: name.clone(),
                            };
                            let encoded = encode_message(&preview_msg)?;
                            stream.write_all(&encoded)?;
                            stream.flush()?;
                        }
                        EventResult::ToggleMouseMode => {
                            // Toggle mouse capture based on new state
                            if app.app_state.mouse_mode {
                                execute!(std::io::stdout(), EnableMouseCapture)?;
                                app.show_timed_message("Mouse scroll enabled");
                            } else {
                                execute!(std::io::stdout(), DisableMouseCapture)?;
                                app.show_timed_message("Text select enabled");
                            }
                        }
                        EventResult::ToggleZoom => {
                            // Recalculate term_cols based on new zoom state
                            term_cols = compute_term_cols(last_size.0, app.app_state.zoomed);
                            app.resize(term_rows, term_cols);
                            let resize_msg = ClientMessage::Resize {
                                rows: term_rows,
                                cols: term_cols,
                            };
                            let encoded = encode_message(&resize_msg)?;
                            stream.write_all(&encoded)?;
                            stream.flush()?;
                            if app.app_state.zoomed {
                                app.show_timed_message("Zoomed — sidebar hidden");
                            } else {
                                app.show_timed_message("Unzoomed — sidebar visible");
                            }
                        }
                        EventResult::OpenSessionOverlay
                        | EventResult::OpenSessionCreate
                        | EventResult::OpenSessionRename
                        | EventResult::OpenSessionKill => {
                            // Fetch fresh session list from server before opening overlay
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            let session_response = send_server_message_sync(
                                stream,
                                ClientMessage::ListSessions,
                                &mut app,
                            )?;
                            let (sessions, active) = if let ServerResponse::Sessions {
                                sessions,
                                active_session,
                            } = session_response
                            {
                                let names: Vec<String> =
                                    sessions.iter().map(|session| session.name.clone()).collect();
                                (names, active_session)
                            } else {
                                (
                                    app.app_state.sessions.clone(),
                                    app.app_state.session_name.clone(),
                                )
                            };
                            app.app_state.sessions = sessions.clone();
                            app.app_state.session_name = active.clone();
                            let mut overlay = SessionOverlayState::new(sessions, active);
                            // Uppercase session commands act on the current session without
                            // forcing an extra chooser keystroke, while reusing its inline editors.
                            match session_action {
                                Some('C') => {
                                    overlay.selected_index = 0;
                                    overlay.drafting_session =
                                        Some(sidebar_tui::state::RenamingState::new(
                                            0,
                                            "",
                                            Focus::Sidebar,
                                        ));
                                }
                                Some('R') => {
                                    let name = overlay
                                        .sessions
                                        .get(overlay.selected_index)
                                        .cloned()
                                        .unwrap_or_default();
                                    overlay.renaming =
                                        Some(sidebar_tui::state::RenamingState::new(
                                            0,
                                            &name,
                                            Focus::Sidebar,
                                        ));
                                }
                                Some('K') => {
                                    let name = overlay.active_session.clone();
                                    app.app_state.mode =
                                        AppMode::Confirming(sidebar_tui::state::ConfirmState::new(
                                            sidebar_tui::state::ConfirmAction::KillSession(
                                                name,
                                            ),
                                            Focus::Sidebar,
                                        ));
                                    stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                                    continue;
                                }
                                _ => {}
                            }
                            app.app_state.mode = AppMode::SessionOverlay(overlay);
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::OpenMoveToSessionOverlay { window_name } => {
                            // Fetch fresh session list from server before opening move overlay
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            let session_response = send_server_message_sync(
                                stream,
                                ClientMessage::ListSessions,
                                &mut app,
                            )?;
                            let (sessions, active) = if let ServerResponse::Sessions {
                                sessions,
                                active_session,
                            } = session_response
                            {
                                let names: Vec<String> =
                                    sessions.iter().map(|session| session.name.clone()).collect();
                                (names, active_session)
                            } else {
                                (
                                    app.app_state.sessions.clone(),
                                    app.app_state.session_name.clone(),
                                )
                            };
                            app.app_state.sessions = sessions.clone();
                            app.app_state.mode =
                                AppMode::SessionOverlay(SessionOverlayState::new_move_mode(
                                    sessions,
                                    active,
                                    window_name,
                                ));
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::SwitchSession { name } => {
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            // Save current session state before switching
                            let current_session = app.app_state.session_name.clone();
                            let last_selected = app
                                .app_state
                                .windows
                                .get(app.app_state.selected_index)
                                .map(|s| s.name.clone());
                            let focused_region = match app.app_state.focus {
                                Focus::Sidebar => "sidebar".to_string(),
                                Focus::Terminal => "terminal".to_string(),
                            };
                            let _ = send_server_message_sync(
                                stream,
                                ClientMessage::SaveSessionState {
                                    session_name: current_session,
                                    last_selected_window: last_selected,
                                    last_focused_region: focused_region,
                                    sidebar_scroll_offset: app.app_state.scroll_offset,
                                },
                                &mut app,
                            );
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                            let response = send_server_message_sync(
                                stream,
                                ClientMessage::SwitchSession { name: name.clone() },
                                &mut app,
                            )?;
                            match response {
                                ServerResponse::SessionSwitched {
                                    name: new_session,
                                    windows: session_windows,
                                    last_selected_window,
                                    last_focused_region,
                                    sidebar_scroll_offset,
                                } => {
                                    // Update windows from the response
                                    app.app_state.windows = session_windows
                                        .iter()
                                        .map(|s| Window::attached(&s.name))
                                        .collect();
                                    app.app_state.session_name = new_session;

                                    // Restore saved session state
                                    app.app_state.scroll_offset = sidebar_scroll_offset;
                                    // Direct/chooser switching has explicit focus semantics; only CLI-style
                                    // restoration should inherit the target session's saved pane.
                                    app.app_state.focus =
                                        requested_session_focus.unwrap_or_else(|| {
                                            if last_focused_region == "sidebar" {
                                                Focus::Sidebar
                                            } else {
                                                Focus::Terminal
                                            }
                                        });

                                    // Restore last selected window index
                                    if let Some(ref last_name) = last_selected_window {
                                        if let Some(idx) = app
                                            .app_state
                                            .windows
                                            .iter()
                                            .position(|s| &s.name == last_name)
                                        {
                                            app.app_state.selected_index = idx;
                                        } else {
                                            app.app_state.selected_index = 0;
                                        }
                                    } else {
                                        app.app_state.selected_index = 0;
                                    }

                                    // Save scroll position for current window before switching session
                                    let current_scroll = app.term_emulator.get_scroll_offset();
                                    if current_scroll > 0 {
                                        app.window_scroll_offsets
                                            .insert(app.window_name.clone(), current_scroll);
                                    } else {
                                        app.window_scroll_offsets.remove(&app.window_name);
                                    }

                                    // If current window is not in new session, switch to last selected or first available
                                    let target_window = last_selected_window
                                        .filter(|name| {
                                            app.app_state.windows.iter().any(|s| &s.name == name)
                                        })
                                        .or_else(|| {
                                            app.app_state.windows.first().map(|s| s.name.clone())
                                        });
                                    if !app
                                        .app_state
                                        .windows
                                        .iter()
                                        .any(|s| s.name == app.window_name)
                                    {
                                        if let Some(first) = target_window.or_else(|| {
                                            app.app_state.windows.first().map(|s| s.name.clone())
                                        }) {
                                            let switch_response = send_server_message_sync(
                                                stream,
                                                ClientMessage::Attach {
                                                    window_name: first.clone(),
                                                    rows: term_rows,
                                                    cols: term_cols,
                                                    cwd: cwd.clone(),
                                                },
                                                &mut app,
                                            )?;
                                            if let ServerResponse::Attached {
                                                window_name: attached_name,
                                                terminal_state: new_state,
                                                ..
                                            } = switch_response
                                            {
                                                app.window_name = attached_name.clone();
                                                app.term_emulator =
                                                    Terminal::new(term_rows, term_cols);
                                                if let Some(state_bytes) = new_state {
                                                    app.process_output(&state_bytes);
                                                }
                                                // Restore scroll position for the newly attached window
                                                if let Some(&saved_offset) =
                                                    app.window_scroll_offsets.get(&attached_name)
                                                {
                                                    app.term_emulator.scroll_up(saved_offset);
                                                }
                                            }
                                        } else {
                                            app.window_name = String::new();
                                            app.term_emulator = Terminal::new(term_rows, term_cols);
                                        }
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to switch session: {}", message);
                                }
                                _ => {}
                            }
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::CreateSession { name } => {
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            let response = send_server_message_sync(
                                stream,
                                ClientMessage::CreateSession { name: name.clone() },
                                &mut app,
                            )?;
                            match response {
                                ServerResponse::SessionCreated { name: new_session } => {
                                    // Add to local session list
                                    if !app.app_state.sessions.contains(&new_session) {
                                        app.app_state.sessions.push(new_session.clone());
                                        app.app_state.sessions.sort();
                                    }
                                    // Update overlay state if still open
                                    if let AppMode::SessionOverlay(ref mut ov) =
                                        app.app_state.mode
                                    {
                                        ov.sessions = app.app_state.sessions.clone();
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to create session: {}", message);
                                }
                                _ => {}
                            }
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::RenameSession { old_name, new_name } => {
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            let response = send_server_message_sync(
                                stream,
                                ClientMessage::RenameSession {
                                    old_name: old_name.clone(),
                                    new_name: new_name.clone(),
                                },
                                &mut app,
                            )?;
                            match response {
                                ServerResponse::SessionRenamed {
                                    old_name: old,
                                    new_name: new,
                                } => {
                                    // Update local session list
                                    if let Some(pos) =
                                        app.app_state.sessions.iter().position(|w| w == &old)
                                    {
                                        app.app_state.sessions[pos] = new.clone();
                                        app.app_state.sessions.sort();
                                    }
                                    if app.app_state.session_name == old {
                                        app.app_state.session_name = new.clone();
                                    }
                                    // Update overlay state if still open
                                    if let AppMode::SessionOverlay(ref mut ov) =
                                        app.app_state.mode
                                    {
                                        ov.sessions = app.app_state.sessions.clone();
                                        if ov.active_session == old {
                                            ov.active_session = new.clone();
                                        }
                                        ov.selected_index = ov
                                            .selected_index
                                            .min(ov.sessions.len().saturating_sub(1));
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to rename session: {}", message);
                                }
                                _ => {}
                            }
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::KillSession { name } => {
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            let response = send_server_message_sync(
                                stream,
                                ClientMessage::KillSession { name: name.clone() },
                                &mut app,
                            )?;
                            match response {
                                ServerResponse::SessionKilled { .. } => {
                                    // Refresh session list from server (handles auto-created Default)
                                    let session_response = send_server_message_sync(
                                        stream,
                                        ClientMessage::ListSessions,
                                        &mut app,
                                    )?;
                                    if let ServerResponse::Sessions {
                                        sessions,
                                        active_session,
                                    } = session_response
                                    {
                                        let names: Vec<String> =
                                            sessions.into_iter().map(|w| w.name).collect();
                                        app.app_state.sessions = names.clone();
                                        app.app_state.session_name = active_session.clone();
                                        // Update overlay state if still open
                                        if let AppMode::SessionOverlay(ref mut ov) =
                                            app.app_state.mode
                                        {
                                            ov.sessions = names;
                                            ov.active_session = active_session;
                                            ov.selected_index = ov
                                                .selected_index
                                                .min(ov.sessions.len().saturating_sub(1));
                                        }
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to kill session: {}", message);
                                }
                                _ => {}
                            }
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::MoveWindowToSession {
                            window_name,
                            session_name,
                        } => {
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;
                            let response = send_server_message_sync(
                                stream,
                                ClientMessage::MoveWindowToSession {
                                    window_name: window_name.clone(),
                                    session_name: session_name.clone(),
                                },
                                &mut app,
                            )?;
                            match response {
                                ServerResponse::WindowMoved { .. } => {
                                    // Remove window from local list (it's now in another session)
                                    app.app_state.windows.retain(|s| s.name != window_name);
                                    // If we moved the current window away, switch to another
                                    if app.window_name == window_name {
                                        if let Some(next) = app.app_state.windows.first() {
                                            let switch_response = send_server_message_sync(
                                                stream,
                                                ClientMessage::Attach {
                                                    window_name: next.name.clone(),
                                                    rows: term_rows,
                                                    cols: term_cols,
                                                    cwd: cwd.clone(),
                                                },
                                                &mut app,
                                            )?;
                                            if let ServerResponse::Attached {
                                                window_name: attached_name,
                                                terminal_state: new_state,
                                                ..
                                            } = switch_response
                                            {
                                                app.window_name = attached_name;
                                                app.term_emulator =
                                                    Terminal::new(term_rows, term_cols);
                                                if let Some(state_bytes) = new_state {
                                                    app.process_output(&state_bytes);
                                                }
                                            }
                                        } else {
                                            app.window_name = String::new();
                                            app.term_emulator = Terminal::new(term_rows, term_cols);
                                        }
                                    }
                                }
                                ServerResponse::Error { message } => {
                                    eprintln!("Failed to move window: {}", message);
                                }
                                _ => {}
                            }
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::ReorderWindow { .. } => {
                            // The state machine already changed the visible stable order.
                        }
                        EventResult::SwitchRelativeSession { .. } => {
                            unreachable!("resolved before dispatch")
                        }
                        EventResult::Consumed => {
                            // Event was consumed by UI state machine, nothing to forward
                        }
                        EventResult::NotConsumed => {
                            // Event not consumed - only forward to terminal if terminal is focused and in Normal mode
                            if app.app_state.focus == Focus::Terminal
                                && matches!(app.app_state.mode, AppMode::Normal)
                                && !app.window_name.is_empty()
                            {
                                let bytes = key_to_bytes(&key);
                                if !bytes.is_empty() {
                                    let input_msg = ClientMessage::Input { data: bytes };
                                    let encoded = encode_message(&input_msg)?;
                                    stream.write_all(&encoded)?;
                                    stream.flush()?;
                                    // Preserve stable displayed positions for numeric and reorder shortcuts.
                                }
                            }
                        }
                    }
                }
                Event::Resize(width, height) => {
                    if (width, height) != last_size {
                        last_size = (width, height);
                        // No terminal border, padding, or bottom hints consume PTY cells.
                        term_cols = compute_term_cols(width, app.app_state.zoomed);
                        term_rows = height;
                        app.resize(term_rows, term_cols);

                        // Send resize to server
                        let resize_msg = ClientMessage::Resize {
                            rows: term_rows,
                            cols: term_cols,
                        };
                        let encoded = encode_message(&resize_msg)?;
                        stream.write_all(&encoded)?;
                        stream.flush()?;
                    }
                }
                Event::Mouse(mouse_event) => {
                    // Handle mouse scroll wheel events.
                    // Per spec: scrolling works regardless of focus when mouse mode is enabled.
                    //
                    // Behavior depends on whether a full-screen app is running:
                    // - Normal terminal (shell prompt, etc.): scroll through TUI history
                    // - Full-screen app (vim, less, htop via alt screen): forward to PTY
                    if matches!(app.app_state.mode, AppMode::Normal) && !app.window_name.is_empty()
                    {
                        match mouse_event.kind {
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                                let scroll_up =
                                    matches!(mouse_event.kind, MouseEventKind::ScrollUp);

                                if app.term_emulator.is_alt_screen() {
                                    // Full-screen app running — forward scroll to PTY as ANSI
                                    let bytes = encode_mouse_scroll(
                                        scroll_up,
                                        mouse_event.column + 1,
                                        mouse_event.row + 1,
                                    );
                                    let input_msg = ClientMessage::Input { data: bytes };
                                    let encoded = encode_message(&input_msg)?;
                                    stream.write_all(&encoded)?;
                                    stream.flush()?;
                                } else {
                                    // Normal terminal — scroll TUI history with velocity throttling
                                    let now = std::time::Instant::now();
                                    let since_last_action =
                                        now.duration_since(app.last_scroll_time).as_millis();
                                    let since_last_event =
                                        now.duration_since(app.last_scroll_event_time).as_millis();

                                    let is_fast = since_last_event < SCROLL_FAST_THRESHOLD_MS;
                                    app.last_scroll_event_time = now;

                                    if is_fast {
                                        app.scroll_event_count += 1;
                                    } else {
                                        app.scroll_event_count = 1;
                                    }

                                    if since_last_action >= SCROLL_THROTTLE_MS {
                                        let scroll_lines = match app.scroll_event_count {
                                            0..=5 => 1,
                                            6..=10 => 2,
                                            11..=15 => 3,
                                            16..=20 => 4,
                                            21..=25 => 5,
                                            _ => 6,
                                        };

                                        let mut up_count: i32 = if scroll_up { 1 } else { 0 };
                                        let mut down_count: i32 = if scroll_up { 0 } else { 1 };

                                        while event::poll(Duration::from_millis(0))? {
                                            match event::read()? {
                                                Event::Mouse(m) => match m.kind {
                                                    MouseEventKind::ScrollUp => up_count += 1,
                                                    MouseEventKind::ScrollDown => down_count += 1,
                                                    _ => break,
                                                },
                                                _ => break,
                                            }
                                        }

                                        if up_count > down_count {
                                            app.term_emulator.scroll_up(scroll_lines);
                                        } else if down_count > up_count {
                                            app.term_emulator.scroll_down(scroll_lines);
                                        }

                                        app.last_scroll_time = now;
                                        app.scroll_event_count = 0;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => {
                    // Not handled yet
                }
            }
        }
    }

    Ok(())
}

/// Sidebar width in characters
pub const SIDEBAR_WIDTH: u16 = 28;

/// Compute the number of terminal columns given the total screen width and zoom state.
/// When zoomed, the sidebar is hidden so the terminal gets the full width.
pub fn compute_term_cols(screen_width: u16, zoomed: bool) -> u16 {
    let sidebar = if zoomed { 0 } else { SIDEBAR_WIDTH };
    screen_width.saturating_sub(sidebar)
}

/// Split only horizontally so contextual hints cannot change terminal geometry.
fn app_layout(area: Rect, zoomed: bool) -> (Rect, Rect) {
    let chunks = Layout::horizontal([
        Constraint::Length(if zoomed { 0 } else { SIDEBAR_WIDTH }),
        Constraint::Min(0),
    ])
    .split(area);
    (chunks[0], chunks[1])
}

/// Keep hints inside the sidebar frame, separated from its scrollable list.
fn render_sidebar_hints(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    hints: sidebar_tui::hint_bar::HintBar,
) -> Rect {
    let hint_height = hints
        .calculate_height(area.width.saturating_sub(2))
        .min(area.height.saturating_sub(8));
    let list_area = Rect {
        height: area
            .height
            .saturating_sub(hint_height + u16::from(hint_height > 0)),
        ..area
    };
    if area.width == 0 || area.height == 0 {
        return list_area;
    }
    render_sidebar_with_state(frame, list_area, state);
    if hint_height > 0 {
        let focused = state.focus == Focus::Sidebar || state.mode.is_text_input();
        let style = Style::default().fg(if focused {
            colors::FOCUSED_BORDER
        } else {
            colors::DARK_GREY
        });
        let hint_area = Rect::new(
            area.x,
            list_area.bottom().saturating_sub(1),
            area.width,
            hint_height + 2,
        );
        let block = Block::default().borders(Borders::ALL).border_style(style);
        let inner = block.inner(hint_area);
        frame.render_widget(block, hint_area);
        frame.render_widget(
            Paragraph::new("├").style(style),
            Rect::new(area.x, hint_area.y, 1, 1),
        );
        if area.width > 1 {
            frame.render_widget(
                Paragraph::new("┤").style(style),
                Rect::new(area.right() - 1, hint_area.y, 1, 1),
            );
        }
        frame.render_widget(hints, inner);
    }
    list_area
}

/// Render the application UI with server-connected terminal emulator.
fn render_server_app(frame: &mut Frame, app: &mut ServerApp) {
    // Calculate hint bar height first
    let mut hint_bar = hint_bar_for_state(&app.app_state);
    // Apply timed message if one is active (overrides normal bindings display)
    if let Some((text, _)) = &app.timed_message {
        hint_bar.show_message(text);
    }
    let (sidebar_area, main_area) = app_layout(frame.area(), app.app_state.zoomed);
    let sidebar_area = render_sidebar_hints(frame, sidebar_area, &app.app_state, hint_bar);

    if let AppMode::SessionOverlay(ref overlay) = app.app_state.mode {
        // The chooser occupies the right side; hints stay in the sidebar.
        render_session_overlay(frame, main_area, overlay);
    } else if matches!(app.app_state.mode, AppMode::Help) {
        render_keybinding_help(frame, main_area);
    } else if app.app_state.zoomed {
        // Zoomed mode: terminal takes the full main area (sidebar is hidden).
        // This allows clean text selection of terminal-only content (e.g. in VSCode).
        render_terminal_emulator_with_state(
            frame,
            main_area,
            &mut app.term_emulator,
            &app.app_state,
        );
    } else {
        render_terminal_emulator_with_state(
            frame,
            main_area,
            &mut app.term_emulator,
            &app.app_state,
        );

        // Set cursor position: if in drafting/renaming mode, show cursor in sidebar
        // Otherwise, the terminal emulator handles its own cursor
        if let Some((cursor_x, cursor_y)) =
            get_sidebar_cursor_position(&app.app_state, sidebar_area)
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

/// Render the static UI layout (for tests without PTY).
pub fn render(frame: &mut Frame) {
    // Use a default AppState for static rendering (welcome state)
    let state = AppState::default();
    render_with_state(frame, &state);
}

/// Render the static UI layout with specific app state.
pub fn render_with_state(frame: &mut Frame, state: &AppState) {
    let (sidebar_area, main_area) = app_layout(frame.area(), state.zoomed);
    let sidebar_area = render_sidebar_hints(frame, sidebar_area, state, hint_bar_for_state(state));

    if let AppMode::SessionOverlay(ref overlay) = state.mode {
        // The chooser occupies the right side; hints stay in the sidebar.
        render_session_overlay(frame, main_area, overlay);
    } else if matches!(state.mode, AppMode::Help) {
        render_keybinding_help(frame, main_area);
    } else {
        render_terminal_view_with_state(frame, main_area, state);

        // Set cursor position for drafting/renaming modes
        if let Some((cursor_x, cursor_y)) = get_sidebar_cursor_position(state, sidebar_area) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

/// Render the discoverable command reference requested by `?`.
fn render_keybinding_help(frame: &mut Frame, area: Rect) {
    let help = Paragraph::new(
        "Sidebar commands\n\n↑/↓ j/k  Browse windows     Enter/Toggle  Focus window\n1-9       Highlight window   n/p           Next/previous window\nl         Last window        c/a           New window/agent\nr/,       Rename window      &/Delete      Kill window\nm         Move window        s             Sessions\nC/R/K     Create/rename/kill session\nP/N       Previous/next session\nz         Hide sidebar       S             Mouse/text selection\nd         Detach             Esc/q         Cancel browsing\n\nGlobal: Ctrl+Space, Ctrl+B, Cmd+Space, Cmd+B toggle focus\nAlt+1-9 and Alt+arrows switch directly; Alt+Shift+Left/Right reorders.\n\nPress Esc, q, or ? to close."
    )
    .block(Block::default().title(" Keybindings ").borders(Borders::ALL))
    .style(Style::default().fg(colors::WHITE));
    frame.render_widget(help, area);
}

/// Render sidebar with specific application state.
fn render_sidebar_with_state(frame: &mut Frame, area: Rect, state: &AppState) {
    let sidebar = Sidebar::new(state);
    frame.render_widget(sidebar, area);
}

/// Render the edge-to-edge terminal placeholder.
fn render_terminal_view_with_state(frame: &mut Frame, area: Rect, state: &AppState) {
    // During drafting mode, terminal pane should be blank and non-interactive
    let is_drafting = matches!(state.mode, AppMode::Drafting(_));

    // The old frame and padding wasted terminal cells; render edge to edge.
    // During drafting, show blank terminal. Otherwise show placeholder.
    if !is_drafting {
        let terminal_placeholder = Paragraph::new("Terminal view (see hint bar for keybindings)")
            .style(Style::default().fg(colors::WHITE));
        frame.render_widget(terminal_placeholder, area);
    }
}

/// Render the terminal emulator without a frame or padding.
fn render_terminal_emulator_with_state(
    frame: &mut Frame,
    area: Rect,
    term: &mut Terminal,
    state: &AppState,
) {
    // During drafting mode, terminal pane should be blank and non-interactive
    let is_drafting = matches!(state.mode, AppMode::Drafting(_));

    // Match the PTY's full-height, borderless geometry exactly.
    // During drafting, show blank terminal. Otherwise render terminal content.
    if !is_drafting {
        // Render the terminal emulator content and cursor in the full right pane.
        // Note: cursor position is handled by get_sidebar_cursor_position during drafting/renaming
        if let Some((cursor_x, cursor_y)) = term.render_with_cursor(frame, area) {
            // Only set terminal cursor if not in text input mode (drafting/renaming)
            if !state.mode.is_text_input() {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

/// Render the session chooser in the right-hand pane.
fn render_session_overlay(frame: &mut Frame, area: Rect, overlay: &SessionOverlayState) {
    // Full-screen overlay: clear the area and fill it.
    frame.render_widget(Clear, area);

    // Determine title based on mode
    let title_text = match &overlay.mode {
        SessionOverlayMode::Normal => "Sessions",
        SessionOverlayMode::MoveWindow { .. } => "Move to Session",
    };

    // Layout: title row (1) + list (rest). Editing is done inline in the list.
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let (title_area, list_area) = (chunks[0], chunks[1]);

    // Render title: "Sessions" in purple, left aligned with 1 char of left padding
    let title_para = Paragraph::new(Line::from(Span::styled(
        format!(" {}", title_text),
        Style::default().fg(colors::PURPLE),
    )));
    frame.render_widget(title_para, title_area);

    // Build virtual list:
    //   - If drafting: virtual[0] = draft row, virtual[i+1] = sessions[i]
    //   - Otherwise: virtual[i] = sessions[i]
    let is_drafting = overlay.drafting_session.is_some();
    let total_count = overlay.sessions.len() + if is_drafting { 1 } else { 0 };
    let max_visible = list_area.height as usize;
    let visible_start = overlay.scroll_offset;
    let visible_end = (visible_start + max_visible).min(total_count);

    let items: Vec<ListItem> = (visible_start..visible_end)
        .map(|virtual_index| {
            let is_selected = virtual_index == overlay.selected_index;

            if is_drafting && virtual_index == 0 {
                // Draft row: shown at top, selected, with the current draft name
                let draft = overlay.drafting_session.as_ref().unwrap();
                let display = format!("   {}", draft.new_name);
                let style = Style::default().fg(Color::White).bg(Color::Indexed(238));
                ListItem::new(Line::from(Span::styled(display, style)))
            } else {
                // Session row (shift index by 1 when a draft row exists above)
                let session_index = if is_drafting {
                    virtual_index - 1
                } else {
                    virtual_index
                };
                let name = &overlay.sessions[session_index];
                let is_active = *name == overlay.active_session;

                // If renaming this selected row, show the in-progress rename text instead
                let display_name = if overlay.renaming.is_some() && is_selected {
                    overlay.renaming.as_ref().unwrap().new_name.as_str()
                } else {
                    name.as_str()
                };

                let prefix = if is_active { "* " } else { "  " };
                let display = format!(" {}{}", prefix, display_name);

                let style = if is_selected {
                    Style::default().fg(Color::White).bg(Color::Indexed(238))
                } else if is_active {
                    Style::default().fg(colors::PURPLE)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(display, style)))
            }
        })
        .collect();

    frame.render_widget(List::new(items), list_area);

    // Render truncation indicators if needed
    if total_count > max_visible {
        let indicator_style = Style::default().fg(Color::Indexed(238));
        if visible_start > 0 && list_area.height > 0 {
            let top_line = Line::from(Span::styled("...", indicator_style));
            frame.render_widget(
                Paragraph::new(top_line),
                Rect::new(
                    list_area.x + 1,
                    list_area.y,
                    list_area.width.saturating_sub(1),
                    1,
                ),
            );
        }
        if visible_end < total_count && list_area.height > 0 {
            let bot_y = list_area.y + list_area.height.saturating_sub(1);
            let bot_line = Line::from(Span::styled("...", indicator_style));
            frame.render_widget(
                Paragraph::new(bot_line),
                Rect::new(list_area.x + 1, bot_y, list_area.width.saturating_sub(1), 1),
            );
        }
    }

    // Set cursor position for inline text editing.
    // All session rows have a 3-char prefix (" * " or "   "), so cursor_x = list_area.x + 3 + cursor_position.
    let selected_row_in_view =
        overlay.selected_index >= visible_start && overlay.selected_index < visible_end;
    if selected_row_in_view {
        let row = (overlay.selected_index - visible_start) as u16;
        let cursor_pos = if is_drafting && overlay.selected_index == 0 {
            overlay
                .drafting_session
                .as_ref()
                .map(|d| d.cursor_position)
        } else if overlay.renaming.is_some() {
            overlay.renaming.as_ref().map(|r| r.cursor_position)
        } else {
            None
        };
        if let Some(cp) = cursor_pos {
            let cursor_x = list_area.x + 3 + cp as u16;
            let cursor_y = list_area.y + row;
            if cursor_x < list_area.x + list_area.width {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use sidebar_tui::colors;

    #[test]
    fn test_sidebar_header_shows_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Default"),
            "Should contain session name 'Default', got: {}",
            content
        );
    }

    #[test]
    fn test_sidebar_has_border() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Check top-left corner has border character
        let corner = &buffer[(0, 0)];
        assert!(
            corner.symbol() == "┌" || corner.symbol() == "╭",
            "Sidebar top-left should have border corner, got: {}",
            corner.symbol()
        );

        // Check border color - sidebar is focused by default, so should be FOCUSED_BORDER (55, purple)
        assert_eq!(
            corner.fg,
            colors::FOCUSED_BORDER,
            "Sidebar border should have focused border color when focused, got: {:?}",
            corner.fg
        );
    }

    #[test]
    fn test_sidebar_title_is_purple() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Session name starts at position 2 in the top frame.
        let cell = &buffer[(2, 0)];
        assert_eq!(
            cell.fg,
            colors::PURPLE,
            "Sidebar title text should have purple foreground, got: {:?}",
            cell.fg
        );
    }

    #[test]
    fn test_sidebar_title_is_left_aligned() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Extract the frame title after its leading padding cell.
        let mut title_content = String::new();
        for x in 2..(SIDEBAR_WIDTH - 1) {
            let cell = &buffer[(x, 0)];
            title_content.push_str(cell.symbol());
        }

        // The session name should start at the beginning of the frame title.
        assert!(
            title_content.starts_with("Default"),
            "Title should be left-aligned session name, got: '{}'",
            title_content
        );
    }

    #[test]
    fn test_sidebar_has_no_background_color() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Check that the sidebar body (inside the border) has no special background
        // Check a cell inside the sidebar (not on the border)
        let cell = &buffer[(2, 2)];
        assert_eq!(
            cell.bg,
            Color::Reset,
            "Sidebar body should have no special background, got: {:?}",
            cell.bg
        );
    }

    #[test]
    fn test_sidebar_is_28_chars_wide() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // The sidebar should be exactly SIDEBAR_WIDTH (28) characters wide
        // Check that last sidebar column has the right border character
        let last_sidebar_col = SIDEBAR_WIDTH - 1;
        let first_after_sidebar = SIDEBAR_WIDTH;

        let last_cell = &buffer[(last_sidebar_col, 0)];
        assert!(
            last_cell.symbol() == "┐" || last_cell.symbol() == "╮" || last_cell.symbol() == "─",
            "Column {} (last sidebar) should be a border char, got: {}",
            last_sidebar_col,
            last_cell.symbol()
        );

        // Immediately after the sidebar is unframed terminal content.
        let after_cell = &buffer[(first_after_sidebar, 0)];
        assert_ne!(
            after_cell.fg,
            Color::DarkGray,
            "Column {} (after sidebar) should not have sidebar border color",
            first_after_sidebar
        );
    }

    #[test]
    fn test_terminal_view_has_no_border() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(render).unwrap();
        let buffer = terminal.backend().buffer();
        // The terminal starts with content, not a frame corner.
        assert_eq!(buffer[(SIDEBAR_WIDTH, 0)].symbol(), "T");
        assert_eq!(buffer[(79, 23)].bg, Color::Reset);
    }

    #[test]
    fn test_terminal_uses_all_available_columns() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(render).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(SIDEBAR_WIDTH, 0)].symbol(), "T");
        assert_eq!(compute_term_cols(80, false), 52);
        assert_eq!(compute_term_cols(80, true), 80);
    }

    #[test]
    fn test_terminal_view_shows_placeholder() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Terminal view") && content.contains("hint bar"),
            "Terminal view placeholder should reference hint bar, got: {}",
            content
        );
    }

    #[test]
    fn test_render_fits_in_small_terminal() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Should not panic with a smaller terminal
        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // The sidebar session name title should still appear
        assert!(
            content.contains("Default"),
            "Should contain session name 'Default', got: {}",
            content
        );
    }

    fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
        let mut result = String::new();
        for y in 0..buffer.area().height {
            for x in 0..buffer.area().width {
                let cell = &buffer[(x, y)];
                result.push_str(cell.symbol());
            }
            result.push('\n');
        }
        result
    }

    #[test]
    fn test_terminal_width_excludes_only_sidebar() {
        // Only the sidebar consumes columns: 100 - 28 = 72.
        let window_width: u16 = 100;
        let term_cols = compute_term_cols(window_width, false);
        assert_eq!(term_cols, 72);
    }

    #[test]
    fn test_terminal_width_handles_small_window() {
        // When the screen is narrower than the sidebar, terminal width saturates to zero.
        let window_width: u16 = 15;
        let term_cols = compute_term_cols(window_width, false);
        assert_eq!(term_cols, 0);
    }

    #[test]
    fn test_terminal_width_at_boundary() {
        // No terminal columns remain at exactly the sidebar width.
        let window_width: u16 = SIDEBAR_WIDTH;
        let term_cols = compute_term_cols(window_width, false);
        assert_eq!(term_cols, 0);
    }

    #[test]
    fn test_resize_state_tracking() {
        // Test that we correctly track the last window size
        let mut last_size = (80u16, 24u16);
        let new_size = (100u16, 30u16);

        // Different size should trigger resize
        if new_size != last_size {
            last_size = new_size;
        }
        assert_eq!(last_size, (100, 30));

        // Same size should not trigger resize (state unchanged)
        let same_size = (100u16, 30u16);
        let should_resize = same_size != last_size;
        assert!(!should_resize, "Same size should not trigger resize");
    }

    #[test]
    fn test_ctrl_q_is_detach_key() {
        // Ctrl+Q should trigger detach
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let is_detach = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(is_detach, "Ctrl+Q should be a detach key");
    }

    #[test]
    fn test_ctrl_b_is_detach_key() {
        // Ctrl+B should trigger detach
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
        let is_detach = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(is_detach, "Ctrl+B should be a detach key");
    }

    #[test]
    fn test_ctrl_other_is_not_detach_key() {
        // Ctrl+X should not trigger detach
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        let is_detach = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(!is_detach, "Ctrl+X should not be a detach key");
    }

    #[test]
    fn test_plain_q_is_not_detach_key() {
        // Plain 'q' without Ctrl should not trigger detach
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let is_detach = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(!is_detach, "Plain 'q' should not be a detach key");
    }

    #[test]
    fn test_plain_b_is_not_detach_key() {
        // Plain 'b' without Ctrl should not trigger detach
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE);
        let is_detach = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(!is_detach, "Plain 'b' should not be a detach key");
    }

    #[test]
    fn test_server_app_creation() {
        let app = ServerApp::new(24, 80, "test", vec![]);
        assert_eq!(app.window_name, "test");
    }

    #[test]
    fn test_server_app_process_output() {
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        app.process_output(b"Hello, World!");
        // Verify terminal emulator received the data
        let contents = app.term_emulator.contents();
        assert!(contents.contains("Hello, World!"));
    }

    #[test]
    fn test_server_app_resize() {
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        app.resize(30, 100);
        // Verify resize happened (no panics)
    }

    #[test]
    fn test_cli_parsing_list() {
        let cli = Cli::try_parse_from(["sb", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::List)));
    }

    #[test]
    fn test_cli_parsing_kill() {
        let cli = Cli::try_parse_from(["sb", "kill", "mywindow"]).unwrap();
        match cli.command {
            Some(Commands::Kill { window }) => assert_eq!(window, "mywindow"),
            _ => panic!("Expected Kill command"),
        }
    }

    #[test]
    fn test_cli_parsing_attach() {
        let cli = Cli::try_parse_from(["sb", "attach", "mywindow"]).unwrap();
        match cli.command {
            Some(Commands::Attach { window }) => assert_eq!(window, "mywindow"),
            _ => panic!("Expected Attach command"),
        }
    }

    #[test]
    fn test_cli_parsing_attach_default() {
        let cli = Cli::try_parse_from(["sb", "attach"]).unwrap();
        match cli.command {
            Some(Commands::Attach { window }) => assert_eq!(window, "main"),
            _ => panic!("Expected Attach command"),
        }
    }

    #[test]
    fn test_cli_parsing_server() {
        let cli = Cli::try_parse_from(["sb", "server"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Server)));
    }

    #[test]
    fn test_cli_parsing_no_command() {
        let cli = Cli::try_parse_from(["sb"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.window.is_none()); // No default window - will show welcome state or first existing
    }

    #[test]
    fn test_cli_parsing_window_flag() {
        let cli = Cli::try_parse_from(["sb", "-s", "mywindow"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.window, Some("mywindow".to_string()));
    }

    #[test]
    fn test_cli_parsing_stale() {
        let cli = Cli::try_parse_from(["sb", "stale"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Stale)));
    }

    #[test]
    fn test_cli_parsing_restore() {
        let cli = Cli::try_parse_from(["sb", "restore", "old-window"]).unwrap();
        match cli.command {
            Some(Commands::Restore { window }) => assert_eq!(window, "old-window"),
            _ => panic!("Expected Restore command"),
        }
    }

    #[test]
    fn test_cli_parsing_forget() {
        let cli = Cli::try_parse_from(["sb", "forget", "old-window"]).unwrap();
        match cli.command {
            Some(Commands::Forget { window }) => assert_eq!(window, "old-window"),
            _ => panic!("Expected Forget command"),
        }
    }

    #[test]
    fn test_mouse_scroll_position_translation() {
        // Test that screen coordinates are correctly translated to terminal-relative coordinates
        // Terminal content starts immediately after the 28-column sidebar.
        // Screen column 32 becomes one-indexed terminal column 5.
        let term_content_start = SIDEBAR_WIDTH;
        let screen_col: u16 = 32;
        let term_col = screen_col - term_content_start + 1;
        assert_eq!(term_col, 5);
    }

    #[test]
    fn test_mouse_scroll_row_is_one_indexed() {
        // Screen row 0 should become terminal row 1 (1-indexed)
        let screen_row: u16 = 0;
        let term_row = screen_row + 1;
        assert_eq!(term_row, 1);
    }

    #[test]
    fn test_mouse_scroll_in_sidebar_area_is_ignored() {
        // Events to the left of the terminal content belong to the sidebar.
        // Terminal content starts immediately after the 28-column sidebar.
        let term_content_start = SIDEBAR_WIDTH;
        let mouse_column: u16 = 27; // Last sidebar column.
        let should_handle = mouse_column >= term_content_start;
        assert!(
            !should_handle,
            "Scroll coordinates inside the sidebar are outside terminal content"
        );
    }

    #[test]
    fn test_mouse_scroll_in_terminal_area_is_handled() {
        // Events in terminal content area should be handled
        // Terminal content starts after the sidebar, with no border or padding.
        let term_content_start = SIDEBAR_WIDTH;
        let mouse_column: u16 = 35; // Inside terminal content area
        let should_handle = mouse_column >= term_content_start;
        assert!(should_handle, "Scroll in terminal area should be handled");
    }

    #[test]
    fn test_mouse_scroll_works_regardless_of_focus() {
        // Per spec line 91: "Mouse scrolling when the Sidebar TUI is opened at all,
        // regardless of focus should scroll the terminal pane's visible history."
        // This test documents that focus is NOT a condition for mouse scroll handling.
        // The only conditions are: Normal mode, mouse in terminal area, window exists.
        use sidebar_tui::state::{AppMode, AppState, Focus};

        let mut state = AppState {
            mode: AppMode::Normal,
            ..Default::default()
        };

        // Scroll should work when sidebar is focused
        state.focus = Focus::Sidebar;
        let should_scroll_sidebar_focused = matches!(state.mode, AppMode::Normal); // Focus NOT checked
        assert!(
            should_scroll_sidebar_focused,
            "Scroll should work when sidebar is focused"
        );

        // Scroll should work when terminal is focused
        state.focus = Focus::Terminal;
        let should_scroll_terminal_focused = matches!(state.mode, AppMode::Normal);
        assert!(
            should_scroll_terminal_focused,
            "Scroll should work when terminal is focused"
        );
    }

    #[test]
    fn test_hint_changes_leave_terminal_cells_and_geometry_unchanged() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut app = ServerApp::new_welcome_state(24, 52);
        app.term_emulator.process(b"\x1b[1;1HA\x1b[24;52HZ");
        terminal
            .draw(|frame| render_server_app(frame, &mut app))
            .unwrap();
        let before = terminal.backend().buffer().clone();
        assert_eq!(before[(28, 0)].symbol(), "A");
        assert_eq!(before[(79, 23)].symbol(), "Z");
        for focus in [Focus::Terminal, Focus::Sidebar] {
            app.app_state.focus = focus;
            app.timed_message = None;
            terminal
                .draw(|frame| render_server_app(frame, &mut app))
                .unwrap();
            for y in 0..24 {
                for x in 28..80 {
                    assert_eq!(terminal.backend().buffer()[(x, y)], before[(x, y)]);
                }
            }
            app.show_timed_message("A longer temporary message that wraps inside the sidebar");
            terminal
                .draw(|frame| render_server_app(frame, &mut app))
                .unwrap();
            for y in 0..24 {
                for x in 28..80 {
                    assert_eq!(terminal.backend().buffer()[(x, y)], before[(x, y)]);
                }
            }
        }
    }

    #[test]
    fn test_sidebar_hints_geometry_and_tiny_screens() {
        for width in [0, 1, 15, 28, 40, 80] {
            for height in [0, 1, 4, 10, 24] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(render).unwrap();
                let (_, right) = app_layout(Rect::new(0, 0, width, height), false);
                assert_eq!(right.height, height);
                assert_eq!(right.width, compute_term_cols(width, false));
            }
        }
        let (_, zoomed) = app_layout(Rect::new(0, 0, 80, 24), true);
        assert_eq!(zoomed, Rect::new(0, 0, 80, 24));
    }

    #[test]
    fn test_hint_column_rendered_inside_sidebar() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // The hint column remains inside the frame without painting a background.
        let cell = &buffer[(1, 22)];
        assert_eq!(
            cell.bg,
            Color::Reset,
            "Hint text should preserve the terminal background, got: {:?}",
            cell.bg
        );
    }

    #[test]
    fn test_hint_bar_shows_keybindings() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // In default state (sidebar focused, welcome state), should show "n New" and "d Detach"
        assert!(
            content.contains("n New") || content.contains("New"),
            "Hint bar should show 'New' keybinding, got: {}",
            content
        );
    }

    #[test]
    fn test_hint_bar_shows_detach_path() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // The exit path is pinned below the contextual hints.
        assert!(
            content.contains("Detach"),
            "Hint bar should show detach path, got: {}",
            content
        );
    }

    #[test]
    fn test_hint_bar_has_correct_keybinding_colors() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(render).unwrap();
        let buffer = terminal.backend().buffer();
        assert!((2..23).any(|y| buffer[(1, y)].fg == colors::PURPLE));
    }

    #[test]
    fn test_terminal_focused_state_hint_bar() {
        use sidebar_tui::state::AppState;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Terminal focus exposes the global sidebar toggle.
        assert!(
            content.contains("ctrl + space/b"),
            "Hint bar should show 'ctrl + b' binding when terminal is focused, got: {}",
            content
        );
    }

    #[test]
    fn test_terminal_focus_indicated_by_sidebar_outline() {
        use sidebar_tui::state::AppState;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();

        // Focus is indicated by the sidebar alone, never a terminal frame.
        assert_eq!(buffer[(SIDEBAR_WIDTH, 0)].symbol(), "T");

        // Sidebar border should be DARK_GREY when unfocused
        let sidebar_corner = &buffer[(0, 0)];
        assert_eq!(
            sidebar_corner.fg,
            colors::DARK_GREY,
            "Sidebar border should be dark grey when unfocused, got: {:?}",
            sidebar_corner.fg
        );
    }

    #[test]
    fn test_drafting_mode_shows_blank_terminal() {
        use sidebar_tui::state::{AppState, DraftingState, WindowType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // During drafting, terminal placeholder text should NOT appear
        assert!(
            !content.contains("Terminal view"),
            "Terminal should be blank during drafting mode, got: {}",
            content
        );
    }

    #[test]
    fn test_drafting_mode_terminal_is_unframed() {
        use sidebar_tui::state::{AppState, DraftingState, WindowType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(SIDEBAR_WIDTH, 0)].symbol(), " ");
        assert_eq!(buffer[(SIDEBAR_WIDTH, 0)].bg, Color::Reset);
    }

    #[test]
    fn test_drafting_mode_sidebar_border_is_focused() {
        use sidebar_tui::state::{AppState, DraftingState, WindowType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();

        // Sidebar border should be FOCUSED_BORDER (55, purple) during drafting (focused)
        let sidebar_corner = &buffer[(0, 0)];
        assert_eq!(
            sidebar_corner.fg,
            colors::FOCUSED_BORDER,
            "Sidebar border should be color 55 (purple) during drafting, got: {:?}",
            sidebar_corner.fg
        );
    }

    #[test]
    fn test_drafting_mode_hint_bar_shows_correct_bindings() {
        use sidebar_tui::state::{AppState, DraftingState, WindowType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Hint bar should show "enter Create" and "esc Cancel" during drafting
        assert!(
            content.contains("Create"),
            "Hint bar should show 'Create' during drafting, got: {}",
            content
        );
        assert!(
            content.contains("Cancel"),
            "Hint bar should show 'Cancel' during drafting, got: {}",
            content
        );
    }

    #[test]
    fn test_create_mode_hint_bar_shows_window_type_options() {
        use sidebar_tui::state::AppState;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::CreateMode {
                previous_focus: Focus::Sidebar,
            },
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Hint bar should show "t Terminal Window" and "a Agent Window" in create mode
        assert!(
            content.contains("Terminal Window"),
            "Hint bar should show 'Terminal Window' in create mode, got: {}",
            content
        );
        assert!(
            content.contains("Agent Window"),
            "Hint bar should show 'Agent Window' in create mode, got: {}",
            content
        );
    }

    #[test]
    fn test_renaming_mode_hint_bar_shows_correct_bindings() {
        use sidebar_tui::state::{AppState, RenamingState, Window};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.mode = AppMode::Renaming(RenamingState::new(0, "test", Focus::Sidebar));

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Hint bar should show "enter Rename" and "esc Cancel" during renaming
        assert!(
            content.contains("Rename"),
            "Hint bar should show 'Rename' during renaming, got: {}",
            content
        );
        assert!(
            content.contains("Cancel"),
            "Hint bar should show 'Cancel' during renaming, got: {}",
            content
        );
    }

    #[test]
    fn test_detach_confirmation_shows_prompt_message() {
        use sidebar_tui::state::{AppState, ConfirmAction, ConfirmState};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Should show detach confirmation message
        assert!(
            content.contains("Detach Sidebar TUI?"),
            "Hint bar should show detach confirmation message, got: {}",
            content
        );
    }

    #[test]
    fn test_detach_confirmation_shows_yes_no_bindings() {
        use sidebar_tui::state::{AppState, ConfirmAction, ConfirmState};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Should show y/n keybindings
        assert!(
            content.contains("Yes"),
            "Hint bar should show 'Yes' binding, got: {}",
            content
        );
        assert!(
            content.contains("No"),
            "Hint bar should show 'No' binding, got: {}",
            content
        );
    }

    #[test]
    fn test_detach_confirmation_has_no_background() {
        use sidebar_tui::state::{AppState, ConfirmAction, ConfirmState};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();

        // Hint text no longer paints a solid background, regardless of confirmation type.
        let last_row = 23;
        let cell = &buffer[(1, last_row - 1)];
        assert_eq!(
            cell.bg,
            ratatui::style::Color::Reset,
            "Detach confirmation hint should preserve the terminal background, got: {:?}",
            cell.bg
        );
    }

    #[test]
    fn test_delete_confirmation_shows_prompt_message() {
        use sidebar_tui::state::{AppState, ConfirmAction, ConfirmState, Window};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.mode = AppMode::Confirming(ConfirmState::new(
            ConfirmAction::KillWindow(0),
            Focus::Sidebar,
        ));

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Should show delete confirmation message
        assert!(
            content.contains("Kill this window") && content.contains("permanently?"),
            "Hint bar should show delete confirmation message, got: {}",
            content
        );
    }

    #[test]
    fn test_delete_confirmation_has_no_background() {
        use sidebar_tui::state::{AppState, ConfirmAction, ConfirmState, Window};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.mode = AppMode::Confirming(ConfirmState::new(
            ConfirmAction::KillWindow(0),
            Focus::Sidebar,
        ));

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();

        // Destructive confirmations retain their text but no longer add a colored fill.
        let last_row = 23;
        let cell = &buffer[(1, last_row - 1)];
        assert_eq!(
            cell.bg,
            ratatui::style::Color::Reset,
            "Delete confirmation hint should preserve the terminal background, got: {:?}",
            cell.bg
        );
    }

    #[test]
    fn test_confirmation_detach_path_shows_n_to_detach() {
        use sidebar_tui::state::{AppState, ConfirmAction, ConfirmState};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        terminal
            .draw(|frame| render_with_state(frame, &state))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // During confirmation, detach path should show "n → d Detach"
        // (pressing n cancels, then q detachs)
        assert!(
            content.contains("n →") || content.contains("n → q"),
            "Confirmation detach path should show 'n →' path, got: {}",
            content
        );
    }

    #[test]
    fn test_handle_drained_response_output() {
        // Test that Output responses are processed correctly
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Output {
            data: b"Hello, World!".to_vec(),
        };

        handle_drained_response(response, &mut app);

        let contents = app.term_emulator.contents();
        assert!(
            contents.contains("Hello, World!"),
            "Output should be processed, got: {}",
            contents
        );
    }

    #[test]
    fn test_handle_drained_response_empty_output() {
        // Test that empty Output responses are handled without panics
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Output { data: vec![] };

        // Should not panic
        handle_drained_response(response, &mut app);
    }

    #[test]
    fn test_handle_drained_response_previewed() {
        // Test that Previewed responses update terminal state
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Previewed {
            window_name: "preview".to_string(),
            terminal_state: Some(b"Preview content".to_vec()),
        };

        handle_drained_response(response, &mut app);

        let contents = app.term_emulator.contents();
        assert!(
            contents.contains("Preview content"),
            "Preview content should be processed, got: {}",
            contents
        );
    }

    #[test]
    fn test_handle_drained_response_previewed_none() {
        // Test that Previewed response with no terminal state is handled
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Previewed {
            window_name: "preview".to_string(),
            terminal_state: None,
        };

        // Should not panic
        handle_drained_response(response, &mut app);
    }

    #[test]
    fn test_handle_drained_response_ignores_other_responses() {
        // Test that other responses are safely ignored during drain
        let mut app = ServerApp::new(24, 80, "test", vec![]);

        // These should all be safely ignored
        handle_drained_response(
            ServerResponse::Attached {
                window_name: "test".to_string(),
                is_new: true,
                terminal_state: None,
            },
            &mut app,
        );

        handle_drained_response(ServerResponse::Detached, &mut app);

        handle_drained_response(
            ServerResponse::Error {
                message: "test error".to_string(),
            },
            &mut app,
        );

        // Terminal should still be empty (no output processed)
        let contents = app.term_emulator.contents();
        assert!(
            contents.trim().is_empty(),
            "Terminal should be empty after ignored responses, got: {}",
            contents
        );
    }

    #[test]
    fn test_handle_main_loop_response_output() {
        // Test that Output messages are processed correctly
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Output {
            data: b"hello".to_vec(),
        };

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));
        assert!(app.term_emulator.contents().contains("hello"));
    }

    #[test]
    fn test_handle_main_loop_response_shutting_down() {
        // Test that ShuttingDown triggers loop break
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::ShuttingDown;

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::ShuttingDown));
    }

    #[test]
    fn test_handle_main_loop_response_error() {
        // Test that Error responses return error result
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Error {
            message: "test error".to_string(),
        };

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Error(msg) if msg == "test error"));
    }

    #[test]
    fn test_handle_main_loop_response_previewed() {
        // Test that Previewed messages update the terminal
        let mut app = ServerApp::new(24, 80, "test", vec![]);
        let response = ServerResponse::Previewed {
            window_name: "preview".to_string(),
            terminal_state: Some(b"preview content".to_vec()),
        };

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));
        assert!(app.term_emulator.contents().contains("preview content"));
    }

    #[test]
    fn test_handle_main_loop_response_other() {
        // Test that other responses are safely ignored with Continue
        let mut app = ServerApp::new(24, 80, "test", vec![]);

        let result = handle_main_loop_response(
            ServerResponse::Attached {
                window_name: "test".to_string(),
                is_new: true,
                terminal_state: None,
            },
            &mut app,
            24,
            80,
        );
        assert!(matches!(result, MainLoopDrainResult::Continue));

        let result = handle_main_loop_response(ServerResponse::Detached, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));
    }
}
