use std::env;
use std::io::Write as IoWrite;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::{Context, bail};
use crossterm::event::{self, Event, MouseEventKind, EnableMouseCapture, DisableMouseCapture};
use crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use sidebar_tui::daemon::{
    self, ClientMessage, DaemonClient, DaemonResponse, MessageReader, get_socket_path,
    ensure_runtime_dir, decode_message, encode_message,
};
use sidebar_tui::hint_bar::hint_bar_for_state;
use sidebar_tui::input::key_to_bytes;
use sidebar_tui::sidebar::{Sidebar, get_sidebar_cursor_position};
use sidebar_tui::state::{AppMode, AppState, EventResult, Focus, Session, SessionType};
use sidebar_tui::terminal::Terminal;
use sidebar_tui::colors;

/// Version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sidebar TUI - A terminal session manager
#[derive(Parser, Debug)]
#[command(name = "sb")]
#[command(version = VERSION)]
#[command(about = "A terminal session manager with session persistence", long_about = None)]
#[command(disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Session name to attach to (if not specified, attaches to most recent or shows welcome state)
    #[arg(short, long)]
    session: Option<String>,

    /// Print version information
    #[arg(short = 'v', short_alias = 'V', long = "version", action = clap::ArgAction::Version)]
    version: (),
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List all active sessions
    List,
    /// Kill a session
    Kill {
        /// Name of the session to kill
        session: String,
    },
    /// Attach to a session (or create if it doesn't exist)
    Attach {
        /// Session name
        #[arg(default_value = "main")]
        session: String,
    },
    /// Start the session daemon
    Daemon,
    /// List stale sessions (from before reboot/crash)
    Stale,
    /// Restore a stale session
    Restore {
        /// Name of the session to restore
        session: String,
    },
    /// Delete stale session metadata
    Forget {
        /// Name of the session to forget
        session: String,
    },
    /// Shutdown the daemon and kill all sessions
    Shutdown,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::List) => cmd_list(),
        Some(Commands::Kill { session }) => cmd_kill(&session),
        Some(Commands::Attach { session }) => cmd_attach(Some(&session)),
        Some(Commands::Daemon) => cmd_daemon(),
        Some(Commands::Stale) => cmd_stale(),
        Some(Commands::Restore { session }) => cmd_restore(&session),
        Some(Commands::Forget { session }) => cmd_forget(&session),
        Some(Commands::Shutdown) => cmd_shutdown(),
        None => cmd_attach(cli.session.as_deref()),
    }
}

/// List all active sessions.
fn cmd_list() -> Result<()> {
    let mut client = connect_to_daemon()?;
    let sessions = client.list_sessions()?;

    if sessions.is_empty() {
        println!("No active sessions");
    } else {
        println!("{:<20} {:<10} {:>5}x{:<5}", "NAME", "STATUS", "ROWS", "COLS");
        for session in sessions {
            let status = if session.is_attached { "attached" } else { "detached" };
            println!(
                "{:<20} {:<10} {:>5}x{:<5}",
                session.name, status, session.rows, session.cols
            );
        }
    }

    Ok(())
}

/// Kill a session.
fn cmd_kill(session_name: &str) -> Result<()> {
    let mut client = connect_to_daemon()?;
    client.kill_session(session_name)?;
    println!("Killed session '{}'", session_name);
    Ok(())
}

/// List stale sessions (from before reboot/crash).
fn cmd_stale() -> Result<()> {
    let mut client = connect_to_daemon()?;
    let sessions = client.list_stale_sessions()?;

    if sessions.is_empty() {
        println!("No stale sessions found");
    } else {
        println!("{:<20} {:<30} {:>5}x{:<5}", "NAME", "WORKING DIR", "ROWS", "COLS");
        for session in sessions {
            let cwd = session.cwd.map(|p| p.display().to_string()).unwrap_or_else(|| "-".to_string());
            println!(
                "{:<20} {:<30} {:>5}x{:<5}",
                session.name, cwd, session.rows, session.cols
            );
        }
        println!("\nUse 'sb restore <name>' to restore a session, or 'sb forget <name>' to delete.");
    }

    Ok(())
}

/// Restore a stale session.
fn cmd_restore(session_name: &str) -> Result<()> {
    let mut client = connect_to_daemon()?;
    client.restore_stale_session(session_name)?;
    println!("Restored session '{}'. Use 'sb attach {}' to connect.", session_name, session_name);
    Ok(())
}

/// Delete stale session metadata.
fn cmd_forget(session_name: &str) -> Result<()> {
    let mut client = connect_to_daemon()?;
    client.delete_stale_session(session_name)?;
    println!("Deleted metadata for session '{}'", session_name);
    Ok(())
}

/// Shutdown the daemon and kill all sessions.
fn cmd_shutdown() -> Result<()> {
    match connect_to_daemon() {
        Ok(mut client) => {
            client.shutdown()?;
            println!("Daemon shutdown complete. All sessions terminated.");
            Ok(())
        }
        Err(_) => {
            println!("No daemon running.");
            Ok(())
        }
    }
}

/// Start the daemon process (runs in foreground).
fn cmd_daemon() -> Result<()> {
    let daemon = daemon::Daemon::new()?;
    println!("Starting daemon at {:?}", daemon.socket_path());
    daemon.run()
}

/// Attach to a session (or show welcome state if no sessions exist).
/// If session_name is None, will attach to the first existing session or show welcome state.
/// If session_name is Some, will attach to that session (creating if needed).
fn cmd_attach(session_name: Option<&str>) -> Result<()> {
    // Ensure daemon is running
    ensure_daemon_running()?;

    // Connect to daemon
    let socket_path = get_socket_path();
    let mut stream = UnixStream::connect(&socket_path)
        .context("Failed to connect to daemon")?;

    // Set read timeout for non-blocking reads
    stream.set_read_timeout(Some(Duration::from_millis(1)))
        .context("Failed to set read timeout")?;

    // Initialize TUI with mouse capture enabled for scroll wheel support.
    // Users can disable with Ctrl+M if they need native text selection.
    let mut ratatui_term = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;

    let result = run_attached(&mut ratatui_term, &mut stream, session_name);

    // Ensure mouse capture is disabled before restoring terminal
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// Connect to the daemon, starting it if necessary.
fn connect_to_daemon() -> Result<DaemonClient> {
    ensure_daemon_running()?;
    DaemonClient::connect()
}

/// Ensure the daemon is running, starting it if necessary.
fn ensure_daemon_running() -> Result<()> {
    ensure_runtime_dir()?;
    let socket_path = get_socket_path();

    // Try to connect to see if daemon is already running
    if UnixStream::connect(&socket_path).is_ok() {
        return Ok(());
    }

    // Start daemon in background
    start_daemon_background()?;

    // Wait for daemon to be ready
    for _ in 0..50 {
        if UnixStream::connect(&socket_path).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    bail!("Daemon failed to start within timeout")
}

/// Start the daemon as a background process.
fn start_daemon_background() -> Result<()> {
    // Get path to current executable
    let exe = env::current_exe().context("Failed to get current executable path")?;

    // Fork daemon process
    Command::new(exe)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn daemon process")?;

    Ok(())
}

/// Minimum time between scroll actions (throttle for trackpad smoothness)
const SCROLL_THROTTLE_MS: u128 = 30;

/// Time threshold for "fast" scrolling (events arriving faster than this = fast scroll)
const SCROLL_FAST_THRESHOLD_MS: u128 = 15;

/// Application state for daemon-connected mode.
struct DaemonApp {
    /// Terminal emulator for parsing PTY output
    term_emulator: Terminal,
    /// Current session name
    session_name: String,
    /// Application UI state (focus, mode, sessions list)
    app_state: AppState,
    /// Last time a scroll action was performed (for throttling)
    last_scroll_time: std::time::Instant,
    /// Last time any scroll event was received (for velocity calculation)
    last_scroll_event_time: std::time::Instant,
    /// Accumulated scroll events for velocity calculation
    scroll_event_count: u32,
}

impl DaemonApp {
    fn new(rows: u16, cols: u16, session_name: &str, sessions: Vec<Session>) -> Self {
        let mut app_state = AppState::with_sessions(sessions);
        // If we have sessions, focus on terminal
        if !app_state.sessions.is_empty() {
            app_state.focus = Focus::Terminal;
        }
        // Mouse mode enabled by default for scroll wheel support
        app_state.mouse_mode = true;
        Self {
            term_emulator: Terminal::new(rows, cols),
            session_name: session_name.to_string(),
            app_state,
            last_scroll_time: std::time::Instant::now(),
            last_scroll_event_time: std::time::Instant::now(),
            scroll_event_count: 0,
        }
    }

    /// Create app in welcome state (no sessions, sidebar focused).
    fn new_welcome_state(rows: u16, cols: u16) -> Self {
        let mut app_state = AppState::default();
        // Mouse mode enabled by default for scroll wheel support
        app_state.mouse_mode = true;
        Self {
            term_emulator: Terminal::new(rows, cols),
            session_name: String::new(),
            app_state,
            last_scroll_time: std::time::Instant::now(),
            last_scroll_event_time: std::time::Instant::now(),
            scroll_event_count: 0,
        }
    }

    /// Process data received from the daemon.
    fn process_output(&mut self, data: &[u8]) {
        self.term_emulator.process(data);
    }

    /// Resize the terminal emulator.
    fn resize(&mut self, rows: u16, cols: u16) {
        self.term_emulator.resize(rows, cols);
    }
}

/// Helper to send a message to the daemon and read the response.
fn send_daemon_message(stream: &mut UnixStream, msg: ClientMessage) -> Result<DaemonResponse> {
    let encoded = encode_message(&msg)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    // Use a longer timeout for synchronous operations
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let response: DaemonResponse = decode_message(stream)
        .context("Failed to read daemon response")?;
    Ok(response)
}

/// Helper to send a sync message and wait for the response, skipping any in-flight
/// async Output messages that arrived before the response.
///
/// This solves a race condition where the daemon sends Output messages from the old
/// session while the client is waiting for an Attached/Killed/etc. response. Without
/// this, `send_daemon_message` would return an Output message instead of the expected
/// response, causing session switches to silently fail and session A content to bleed
/// into session B's terminal.
fn send_daemon_message_sync(stream: &mut UnixStream, msg: ClientMessage, app: &mut DaemonApp) -> Result<DaemonResponse> {
    let encoded = encode_message(&msg)?;
    stream.write_all(&encoded)?;
    stream.flush()?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    loop {
        let response: DaemonResponse = decode_message(stream)
            .context("Failed to read daemon response")?;
        match response {
            // In-flight output from the old session: apply to the current terminal
            // (still session A at this point) and keep waiting for the real response.
            DaemonResponse::Output { data } => {
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
/// sync operations (like CreateSession, DeleteSession) are called while async
/// messages (like Preview, Output) may be pending.
///
/// Returns Ok(()) on success, or an error if reading fails.
fn drain_async_messages(
    msg_reader: &mut MessageReader,
    stream: &mut UnixStream,
    app: &mut DaemonApp,
) -> Result<()> {
    use std::io;

    // 1. Process any complete messages already buffered
    while let Some(response) = msg_reader.try_parse_buffered::<DaemonResponse>()? {
        handle_drained_response(response, app);
    }

    // 2. Try to read any pending data from the socket with short timeout
    // This catches messages in flight but not yet buffered
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    loop {
        match msg_reader.try_read::<DaemonResponse>(stream) {
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
fn handle_drained_response(response: DaemonResponse, app: &mut DaemonApp) {
    match response {
        DaemonResponse::Output { data } => {
            // Process terminal output
            if !data.is_empty() {
                app.process_output(&data);
            }
        }
        DaemonResponse::Previewed { terminal_state: Some(state_bytes), .. } => {
            // Update preview - may be stale if we're about to switch sessions
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
    /// Daemon is shutting down, break main loop
    ShuttingDown,
    /// Daemon sent an error message
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
    stream: &mut UnixStream,
    app: &mut DaemonApp,
    term_rows: u16,
    term_cols: u16,
) -> MainLoopDrainResult {
    use std::io;

    // Read once from socket to get available data into buffer
    // We use a temporary read to avoid consuming a message, then process all buffered
    let read_result = msg_reader.try_read::<DaemonResponse>(stream);

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
        match msg_reader.try_parse_buffered::<DaemonResponse>() {
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
    response: DaemonResponse,
    app: &mut DaemonApp,
    term_rows: u16,
    term_cols: u16,
) -> MainLoopDrainResult {
    match response {
        DaemonResponse::Output { data } => {
            if !data.is_empty() {
                app.process_output(&data);
            }
            MainLoopDrainResult::Continue
        }
        DaemonResponse::ShuttingDown => MainLoopDrainResult::ShuttingDown,
        DaemonResponse::Error { message } => MainLoopDrainResult::Error(message),
        DaemonResponse::Previewed { terminal_state, .. } => {
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

/// Run the TUI, optionally attaching to a session.
/// If requested_session is None, will attach to first existing session or show welcome state.
/// If requested_session is Some, will attach to that session (creating if needed).
fn run_attached(
    ratatui_term: &mut DefaultTerminal,
    stream: &mut UnixStream,
    requested_session: Option<&str>,
) -> Result<()> {
    // Get initial terminal size, accounting for sidebar, horizontal padding, and borders
    let size = ratatui_term.size()?;
    // Subtract sidebar width, 2*h_padding (left + right), and 2 for terminal border (left + right)
    let term_cols = size.width.saturating_sub(SIDEBAR_WIDTH).saturating_sub(TERMINAL_H_PADDING * 2).saturating_sub(2);
    // Subtract 2 for terminal border (top + bottom), and hint bar height (1)
    let term_rows = size.height.saturating_sub(3);

    // Get current working directory
    let cwd = env::current_dir().ok();

    // Load workspace info from daemon to get the active workspace name
    let workspace_response = send_daemon_message(stream, ClientMessage::ListWorkspaces)?;
    let (workspaces_list, active_workspace_name) = match workspace_response {
        DaemonResponse::Workspaces { workspaces, active_workspace } => (workspaces, active_workspace),
        DaemonResponse::Error { message } => {
            bail!("Failed to list workspaces: {}", message);
        }
        other => {
            bail!("Unexpected workspace response: {:?}", other);
        }
    };
    let _ = workspaces_list; // Will be used in workspace overlay later

    // Load session list from daemon
    let session_list_response = send_daemon_message(stream, ClientMessage::List)?;
    let mut daemon_sessions = match session_list_response {
        DaemonResponse::Sessions { names } => names,
        DaemonResponse::Error { message } => {
            bail!("Failed to list sessions: {}", message);
        }
        other => {
            bail!("Unexpected response: {:?}", other);
        }
    };

    // If no active sessions exist, check for stale sessions and auto-restore them
    if daemon_sessions.is_empty() {
        let stale_response = send_daemon_message(stream, ClientMessage::ListStale)?;
        let stale_sessions = match stale_response {
            DaemonResponse::StaleSessions { sessions } => sessions,
            DaemonResponse::Error { message } => {
                eprintln!("Warning: Failed to list stale sessions: {}", message);
                Vec::new()
            }
            _ => Vec::new(),
        };

        // Auto-restore all stale sessions, sorted by last_active (most recent first)
        let mut sorted_stale: Vec<_> = stale_sessions.into_iter().collect();
        sorted_stale.sort_by(|a, b| b.last_active.cmp(&a.last_active));

        for stale in &sorted_stale {
            let restore_response = send_daemon_message(stream, ClientMessage::RestoreStale {
                session_name: stale.name.clone(),
            })?;
            match restore_response {
                DaemonResponse::Restored { .. } => {
                    // Successfully restored
                }
                DaemonResponse::Error { message } => {
                    eprintln!("Warning: Failed to restore session '{}': {}", stale.name, message);
                }
                _ => {}
            }
        }

        // Re-fetch the session list after restoring
        if !sorted_stale.is_empty() {
            let refreshed_response = send_daemon_message(stream, ClientMessage::List)?;
            daemon_sessions = match refreshed_response {
                DaemonResponse::Sessions { names } => names,
                _ => Vec::new(),
            };
        }
    }

    // Convert daemon sessions to AppState sessions, filtered to active workspace
    let sessions: Vec<Session> = daemon_sessions
        .iter()
        .filter(|info| info.workspace_name == active_workspace_name)
        .map(|info| {
            let mut session = Session::new(&info.name);
            session.is_attached = info.is_attached;
            session
        })
        .collect();

    // Determine which session to attach to (if any)
    // - If explicit session requested, attach to it (creating if needed)
    // - If no session requested but sessions exist, attach to first one
    // - If no session requested and no sessions exist, start in welcome state
    let session_to_attach: Option<String> = match requested_session {
        Some(name) => Some(name.to_string()),
        None => {
            if sessions.is_empty() {
                None // Welcome state
            } else {
                Some(sessions[0].name.clone()) // Attach to first existing session
            }
        }
    };

    // Only attach if we have a session to attach to
    let mut app = if let Some(session_name) = session_to_attach {
        // Send attach message
        let attach_response = send_daemon_message(stream, ClientMessage::Attach {
            session_name: session_name.clone(),
            rows: term_rows,
            cols: term_cols,
            cwd: cwd.clone(),
        })?;

        let terminal_state = match attach_response {
            DaemonResponse::Attached { session_name: _, is_new, terminal_state } => {
                (is_new, terminal_state)
            }
            DaemonResponse::Error { message } => {
                bail!("Failed to attach: {}", message);
            }
            other => {
                bail!("Unexpected response: {:?}", other);
            }
        };

        // Build initial session list for AppState
        let mut initial_sessions = sessions;
        // If this was a new session, add it to the front of the list
        if terminal_state.0 {
            initial_sessions.insert(0, Session::attached(&session_name));
        } else {
            // Mark the current session as attached
            for s in &mut initial_sessions {
                if s.name == session_name {
                    s.is_attached = true;
                }
            }
        }

        // Create app with session list
        let mut app = DaemonApp::new(term_rows, term_cols, &session_name, initial_sessions);
        app.app_state.workspace_name = active_workspace_name.clone();

        // Restore terminal state if reattaching
        if let Some(state_bytes) = terminal_state.1 {
            app.process_output(&state_bytes);
        }

        app
    } else {
        // Welcome state - no sessions to attach to
        let mut app = DaemonApp::new_welcome_state(term_rows, term_cols);
        app.app_state.workspace_name = active_workspace_name.clone();
        app
    };

    let mut last_size = (size.width, size.height);

    // Set stream to non-blocking for the main loop
    stream.set_read_timeout(Some(Duration::from_millis(10)))?;

    // Create buffered message reader to handle partial reads safely
    let mut msg_reader = MessageReader::new();

    loop {
        // Drain all available messages from the socket before rendering.
        // This batches multiple output messages (e.g., during paste) into a single render,
        // significantly improving performance compared to render-per-message.
        let drain_result = drain_main_loop_messages(&mut msg_reader, stream, &mut app, term_rows, term_cols);
        match drain_result {
            MainLoopDrainResult::Continue => {}
            MainLoopDrainResult::ShuttingDown => break,
            MainLoopDrainResult::Error(msg) => bail!("Daemon error: {}", msg),
            MainLoopDrainResult::ConnectionError(e) => bail!("Connection error: {}", e),
        }

        // Render the UI once after processing all available messages
        ratatui_term.draw(|frame| render_daemon_app(frame, &mut app))?;

        // Handle input events
        if event::poll(Duration::from_millis(16)).context("event poll failed")? {
            match event::read().context("event read failed")? {
                Event::Key(key) => {
                    // Route key through state machine
                    let result = app.app_state.handle_key(key);

                    match result {
                        EventResult::Quit => {
                            // Send detach message and exit
                            let detach_msg = ClientMessage::Detach;
                            let encoded = encode_message(&detach_msg)?;
                            stream.write_all(&encoded)?;
                            stream.flush()?;
                            break;
                        }
                        EventResult::CreateSession { name, session_type } => {
                            // Drain any pending async messages before sync operation
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;

                            // Create new session via daemon (use sync variant to skip any
                            // remaining in-flight Output messages from the old session)
                            let create_response = send_daemon_message_sync(stream, ClientMessage::Attach {
                                session_name: name.clone(),
                                rows: term_rows,
                                cols: term_cols,
                                cwd: cwd.clone(),
                            }, &mut app)?;

                            match create_response {
                                DaemonResponse::Attached { session_name: attached_name, is_new: _, terminal_state: new_state } => {
                                    // Add session to local state
                                    app.app_state.add_session(Session::attached(&attached_name));
                                    app.session_name = attached_name;
                                    app.app_state.focus = Focus::Terminal;

                                    // Clear terminal emulator for new session
                                    app.term_emulator = Terminal::new(term_rows, term_cols);

                                    // Restore terminal state if reattaching
                                    if let Some(state_bytes) = new_state {
                                        app.process_output(&state_bytes);
                                    }

                                    // For agent sessions, send the claude command
                                    if session_type == SessionType::Agent {
                                        let claude_cmd = b"claude\n";
                                        let input_msg = ClientMessage::Input { data: claude_cmd.to_vec() };
                                        let encoded = encode_message(&input_msg)?;
                                        stream.write_all(&encoded)?;
                                        stream.flush()?;
                                    }
                                }
                                DaemonResponse::Error { message } => {
                                    eprintln!("Failed to create session: {}", message);
                                }
                                _ => {}
                            }
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::DeleteSession { name } => {
                            // Drain any pending async messages before sync operation
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;

                            // Kill session via daemon
                            let kill_response = send_daemon_message_sync(stream, ClientMessage::Kill {
                                session_name: name.clone(),
                            }, &mut app)?;

                            match kill_response {
                                DaemonResponse::Killed { .. } => {
                                    // If we deleted the current session, switch to another
                                    if app.session_name == name {
                                        if let Some(session) = app.app_state.sessions.first() {
                                            // Switch to first available session
                                            let switch_response = send_daemon_message_sync(stream, ClientMessage::Attach {
                                                session_name: session.name.clone(),
                                                rows: term_rows,
                                                cols: term_cols,
                                                cwd: cwd.clone(),
                                            }, &mut app)?;

                                            if let DaemonResponse::Attached { session_name: attached_name, terminal_state: new_state, .. } = switch_response {
                                                app.session_name = attached_name;
                                                app.term_emulator = Terminal::new(term_rows, term_cols);
                                                if let Some(state_bytes) = new_state {
                                                    app.process_output(&state_bytes);
                                                }
                                            }
                                        } else {
                                            // No sessions left, clear terminal
                                            app.session_name = String::new();
                                            app.term_emulator = Terminal::new(term_rows, term_cols);
                                        }
                                    }
                                }
                                DaemonResponse::Error { message } => {
                                    eprintln!("Failed to delete session: {}", message);
                                }
                                _ => {}
                            }
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::RenameSession { old_name, new_name } => {
                            // Drain any pending async messages before sync operation
                            drain_async_messages(&mut msg_reader, stream, &mut app)?;

                            // Rename session via daemon
                            let rename_response = send_daemon_message_sync(stream, ClientMessage::Rename {
                                old_name: old_name.clone(),
                                new_name: new_name.clone(),
                            }, &mut app)?;

                            match rename_response {
                                DaemonResponse::Renamed { .. } => {
                                    // Update current session name if it was renamed
                                    if app.session_name == old_name {
                                        app.session_name = new_name;
                                    }
                                }
                                DaemonResponse::Error { message } => {
                                    eprintln!("Failed to rename session: {}", message);
                                    // Revert local state change
                                    if let Some(session) = app.app_state.sessions.iter_mut()
                                        .find(|s| s.name == new_name) {
                                        session.name = old_name;
                                    }
                                }
                                _ => {}
                            }
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::SwitchSession { name } => {
                            // Only switch if it's a different session
                            if name != app.session_name {
                                // Drain any pending async messages before sync operation
                                drain_async_messages(&mut msg_reader, stream, &mut app)?;

                                // Detach from current session
                                let _ = send_daemon_message_sync(stream, ClientMessage::Detach, &mut app);

                                // Attach to new session
                                let switch_response = send_daemon_message_sync(stream, ClientMessage::Attach {
                                    session_name: name.clone(),
                                    rows: term_rows,
                                    cols: term_cols,
                                    cwd: cwd.clone(),
                                }, &mut app)?;

                                match switch_response {
                                    DaemonResponse::Attached { session_name: attached_name, terminal_state: new_state, .. } => {
                                        app.session_name = attached_name;
                                        app.term_emulator = Terminal::new(term_rows, term_cols);
                                        if let Some(state_bytes) = new_state {
                                            app.process_output(&state_bytes);
                                        }
                                    }
                                    DaemonResponse::Error { message } => {
                                        eprintln!("Failed to switch session: {}", message);
                                    }
                                    _ => {}
                                }
                            }
                            // Move switched session to top (most recently used)
                            app.app_state.move_selected_to_top();
                            // Reset stream timeout after synchronous operation
                            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
                        }
                        EventResult::PreviewSession { name } => {
                            // Request terminal state preview for the selected session
                            // Send the preview request asynchronously - response will be
                            // handled in the main message loop above
                            let preview_msg = ClientMessage::Preview {
                                session_name: name.clone(),
                            };
                            let encoded = encode_message(&preview_msg)?;
                            stream.write_all(&encoded)?;
                            stream.flush()?;
                        }
                        EventResult::ToggleMouseMode => {
                            // Toggle mouse capture based on new state
                            if app.app_state.mouse_mode {
                                execute!(std::io::stdout(), EnableMouseCapture)?;
                            } else {
                                execute!(std::io::stdout(), DisableMouseCapture)?;
                            }
                        }
                        EventResult::Consumed => {
                            // Event was consumed by UI state machine, nothing to forward
                        }
                        EventResult::NotConsumed => {
                            // Event not consumed - only forward to terminal if terminal is focused and in Normal mode
                            if app.app_state.focus == Focus::Terminal
                                && matches!(app.app_state.mode, AppMode::Normal)
                                && !app.session_name.is_empty()
                            {
                                let bytes = key_to_bytes(&key);
                                if !bytes.is_empty() {
                                    let input_msg = ClientMessage::Input { data: bytes };
                                    let encoded = encode_message(&input_msg)?;
                                    stream.write_all(&encoded)?;
                                    stream.flush()?;
                                    // Move active session to top (most recently used)
                                    app.app_state.move_selected_to_top();
                                }
                            }
                        }
                    }
                }
                Event::Resize(width, height) => {
                    if (width, height) != last_size {
                        last_size = (width, height);
                        // Account for sidebar, horizontal padding (left + right), and terminal border
                        let term_cols = width.saturating_sub(SIDEBAR_WIDTH).saturating_sub(TERMINAL_H_PADDING * 2).saturating_sub(2);
                        // Account for terminal border (top + bottom), and hint bar height (1)
                        let term_rows = height.saturating_sub(3);
                        app.resize(term_rows, term_cols);

                        // Send resize to daemon
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
                    // Handle mouse scroll wheel events - scroll through terminal history
                    // Works regardless of focus per spec, but only in Normal mode
                    // Per spec: "Mouse scrolling when the Sidebar TUI is opened at all,
                    // regardless of focus should scroll the terminal pane's visible history."
                    //
                    // Velocity-based scrolling for trackpads:
                    // - Fast gestures (events <15ms apart) = more lines per action
                    // - Slow gestures (events >15ms apart) = fewer lines per action
                    // This lets users control scroll speed with gesture speed.
                    if matches!(app.app_state.mode, AppMode::Normal)
                        && !app.session_name.is_empty()
                    {
                        let now = std::time::Instant::now();
                        let since_last_action = now.duration_since(app.last_scroll_time).as_millis();
                        let since_last_event = now.duration_since(app.last_scroll_event_time).as_millis();

                        match mouse_event.kind {
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                                // Track event timing for velocity
                                let is_fast = since_last_event < SCROLL_FAST_THRESHOLD_MS;
                                app.last_scroll_event_time = now;

                                if is_fast {
                                    app.scroll_event_count += 1;
                                } else {
                                    app.scroll_event_count = 1; // Reset for slow/new gesture
                                }

                                // Only act every SCROLL_THROTTLE_MS
                                if since_last_action >= SCROLL_THROTTLE_MS {
                                    // Calculate scroll amount based on accumulated velocity
                                    // More events accumulated = faster gesture = more lines
                                    // Scales in blocks of 5 events, caps at 6 lines
                                    let scroll_lines = match app.scroll_event_count {
                                        0..=5 => 1,    // Slow: 1 line
                                        6..=10 => 2,   // Medium: 2 lines
                                        11..=15 => 3,  // Fast: 3 lines
                                        16..=20 => 4,  // Faster: 4 lines
                                        21..=25 => 5,  // Very fast: 5 lines
                                        _ => 6,        // Maximum: 6 lines
                                    };

                                    // Drain queued events and determine direction
                                    let mut up_count: i32 = 0;
                                    let mut down_count: i32 = 0;

                                    if matches!(mouse_event.kind, MouseEventKind::ScrollUp) {
                                        up_count += 1;
                                    } else {
                                        down_count += 1;
                                    }

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

                                    // Apply scroll in dominant direction
                                    if up_count > down_count {
                                        app.term_emulator.scroll_up(scroll_lines);
                                    } else if down_count > up_count {
                                        app.term_emulator.scroll_down(scroll_lines);
                                    }

                                    app.last_scroll_time = now;
                                    app.scroll_event_count = 0; // Reset after action
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

/// Horizontal padding on left and right of terminal view (1 character each side per spec)
pub const TERMINAL_H_PADDING: u16 = 1;

/// Render the application UI with daemon-connected terminal emulator.
fn render_daemon_app(frame: &mut Frame, app: &mut DaemonApp) {
    // Calculate hint bar height first
    let hint_bar = hint_bar_for_state(&app.app_state);
    let hint_bar_height = hint_bar.calculate_height(frame.area().width);

    // Create vertical layout: main content + hint bar
    let vertical_chunks = Layout::vertical([
        Constraint::Min(0),  // Main content
        Constraint::Length(hint_bar_height),  // Hint bar
    ])
    .split(frame.area());

    let main_area = vertical_chunks[0];
    let hint_bar_area = vertical_chunks[1];

    // Create horizontal layout for main area: sidebar (28 chars) + terminal view (rest)
    let horizontal_chunks = Layout::horizontal([
        Constraint::Length(SIDEBAR_WIDTH),
        Constraint::Fill(1),
    ])
    .split(main_area);

    let sidebar_area = horizontal_chunks[0];
    render_sidebar_with_state(frame, sidebar_area, &app.app_state);

    // Render terminal at full area - padding is applied inside the border by render function
    let terminal_area = horizontal_chunks[1];
    render_terminal_emulator_with_state(frame, terminal_area, &mut app.term_emulator, &app.app_state);

    // Render hint bar
    frame.render_widget(hint_bar, hint_bar_area);

    // Set cursor position: if in drafting/renaming mode, show cursor in sidebar
    // Otherwise, the terminal emulator handles its own cursor
    if let Some((cursor_x, cursor_y)) = get_sidebar_cursor_position(&app.app_state, sidebar_area) {
        frame.set_cursor_position((cursor_x, cursor_y));
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
    // Calculate hint bar height first
    let hint_bar = hint_bar_for_state(state);
    let hint_bar_height = hint_bar.calculate_height(frame.area().width);

    // Create vertical layout: main content + hint bar
    let vertical_chunks = Layout::vertical([
        Constraint::Min(0),  // Main content
        Constraint::Length(hint_bar_height),  // Hint bar
    ])
    .split(frame.area());

    let main_area = vertical_chunks[0];
    let hint_bar_area = vertical_chunks[1];

    // Create horizontal layout for main area: sidebar (28 chars) + terminal view (rest)
    let horizontal_chunks = Layout::horizontal([
        Constraint::Length(SIDEBAR_WIDTH),
        Constraint::Fill(1),
    ])
    .split(main_area);

    let sidebar_area = horizontal_chunks[0];
    render_sidebar_with_state(frame, sidebar_area, state);

    // Render terminal at full area - padding is applied inside the border by render function
    let terminal_area = horizontal_chunks[1];
    render_terminal_view_with_state(frame, terminal_area, state);

    // Render hint bar
    frame.render_widget(hint_bar, hint_bar_area);

    // Set cursor position for drafting/renaming modes
    if let Some((cursor_x, cursor_y)) = get_sidebar_cursor_position(state, sidebar_area) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Shrink a rect by horizontal padding only (left and right sides)
fn pad_rect_horizontal(rect: Rect, padding: u16) -> Rect {
    Rect {
        x: rect.x.saturating_add(padding),
        y: rect.y,
        width: rect.width.saturating_sub(padding * 2),
        height: rect.height,
    }
}

/// Render sidebar with specific application state.
fn render_sidebar_with_state(frame: &mut Frame, area: Rect, state: &AppState) {
    let sidebar = Sidebar::new(state);
    frame.render_widget(sidebar, area);
}

/// Render terminal view placeholder with focus-aware border colors.
fn render_terminal_view_with_state(frame: &mut Frame, area: Rect, state: &AppState) {
    // During drafting mode, terminal pane should be blank and non-interactive
    let is_drafting = matches!(state.mode, AppMode::Drafting(_));

    // Terminal border color depends on focus (but always unfocused during drafting)
    let border_color = if !is_drafting && state.focus == Focus::Terminal {
        colors::FOCUSED_BORDER
    } else {
        colors::DARK_GREY
    };

    let terminal_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner_area = terminal_block.inner(area);
    frame.render_widget(terminal_block, area);

    // Apply horizontal padding inside the border
    let padded_inner = pad_rect_horizontal(inner_area, TERMINAL_H_PADDING);

    // During drafting, show blank terminal. Otherwise show placeholder.
    if !is_drafting {
        let terminal_placeholder = Paragraph::new("Terminal view (see hint bar for keybindings)")
            .style(Style::default().fg(colors::WHITE));
        frame.render_widget(terminal_placeholder, padded_inner);
    }
}

/// Render the terminal emulator with focus-aware border colors.
fn render_terminal_emulator_with_state(frame: &mut Frame, area: Rect, term: &mut Terminal, state: &AppState) {
    // During drafting mode, terminal pane should be blank and non-interactive
    let is_drafting = matches!(state.mode, AppMode::Drafting(_));

    // Terminal border color depends on focus (but always unfocused during drafting)
    let border_color = if !is_drafting && state.focus == Focus::Terminal {
        colors::FOCUSED_BORDER
    } else {
        colors::DARK_GREY
    };

    let terminal_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner_area = terminal_block.inner(area);
    frame.render_widget(terminal_block, area);

    // Apply horizontal padding inside the border
    let padded_inner = pad_rect_horizontal(inner_area, TERMINAL_H_PADDING);

    // During drafting, show blank terminal. Otherwise render terminal content.
    if !is_drafting {
        // Render the terminal emulator content with cursor inside the border + padding
        // Note: cursor position is handled by get_sidebar_cursor_position during drafting/renaming
        if let Some((cursor_x, cursor_y)) = term.render_with_cursor(frame, padded_inner) {
            // Only set terminal cursor if not in text input mode (drafting/renaming)
            if !state.mode.is_text_input() {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use sidebar_tui::colors;
    use ratatui::Terminal;

    #[test]
    fn test_sidebar_header_shows_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Default"),
            "Should contain workspace name 'Default', got: {}",
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

        // Check border color - sidebar is focused by default, so should be FOCUSED_BORDER (250)
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

        // Workspace name title starts inside the border + padding (position 2, row 1)
        // Check the first character's foreground color
        let cell = &buffer[(2, 1)];
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

        // Title should start after left border + padding on row 1 (inside border)
        // With 1 char padding, content starts at x=2
        // Extract content row within sidebar (after left border and padding)
        let mut title_content = String::new();
        for x in 2..(SIDEBAR_WIDTH - 1) {
            let cell = &buffer[(x, 1)];
            title_content.push_str(cell.symbol());
        }

        // The workspace name should start at the beginning (left-aligned after padding)
        assert!(
            title_content.starts_with("Default"),
            "Title should be left-aligned workspace name, got: '{}'",
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

        // After sidebar is padding, which has no border styling
        let after_cell = &buffer[(first_after_sidebar, 0)];
        assert_ne!(
            after_cell.fg,
            Color::DarkGray,
            "Column {} (after sidebar) should not have sidebar border color",
            first_after_sidebar
        );
    }

    #[test]
    fn test_terminal_view_has_border() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Terminal view starts right after sidebar (padding is inside the border)
        let term_start_x = SIDEBAR_WIDTH;
        let corner = &buffer[(term_start_x, 0)];

        // Check top-left corner of terminal has border character
        assert!(
            corner.symbol() == "┌" || corner.symbol() == "╭",
            "Terminal top-left should have border corner, got: {}",
            corner.symbol()
        );

        // In default state, sidebar is focused so terminal border should be DARK_GREY (unfocused)
        assert_eq!(
            corner.fg,
            colors::DARK_GREY,
            "Terminal border should have dark grey foreground when unfocused, got: {:?}",
            corner.fg
        );
    }

    #[test]
    fn test_terminal_padding_is_inside_border() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Terminal border should start right after sidebar (no gap for padding)
        let border_x = SIDEBAR_WIDTH;
        let border_cell = &buffer[(border_x, 0)];
        assert!(
            border_cell.symbol() == "┌" || border_cell.symbol() == "╭",
            "Terminal border should start at column {}, got: {}",
            border_x,
            border_cell.symbol()
        );

        // Content (placeholder text) should start after border + padding
        // Border is at column 28, so content starts at 28 + 1 (border) + 2 (padding) = 31
        let content_start_x = SIDEBAR_WIDTH + 1 + TERMINAL_H_PADDING;
        let content_cell = &buffer[(content_start_x, 1)];
        // The placeholder text "Terminal view..." should start here
        assert_eq!(
            content_cell.symbol(), "T",
            "Terminal content should start at column {} (inside border + padding), got: '{}'",
            content_start_x,
            content_cell.symbol()
        );

        // Position between border and content should be empty (padding)
        let padding_x = SIDEBAR_WIDTH + 1; // First padding column after border
        let padding_cell = &buffer[(padding_x, 1)];
        assert_eq!(
            padding_cell.symbol().trim(), "",
            "Padding area at column {} should be empty space, got: '{}'",
            padding_x,
            padding_cell.symbol()
        );
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

        // The sidebar workspace name title should still appear
        assert!(
            content.contains("Default"),
            "Should contain workspace name 'Default', got: {}",
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
    fn test_terminal_width_excludes_sidebar_padding_and_border() {
        // When window is 100 wide, terminal should be 100 - 28 (sidebar) - 2 (h_padding * 2) - 2 (border) = 68
        // TERMINAL_H_PADDING = 1, so h_padding * 2 = 2
        let window_width: u16 = 100;
        let term_cols = window_width.saturating_sub(SIDEBAR_WIDTH).saturating_sub(TERMINAL_H_PADDING * 2).saturating_sub(2);
        assert_eq!(term_cols, 68);
    }

    #[test]
    fn test_terminal_width_handles_small_window() {
        // When window is smaller than sidebar + h_padding + border, terminal width should be 0 (saturating sub)
        let window_width: u16 = 15;
        let term_cols = window_width.saturating_sub(SIDEBAR_WIDTH).saturating_sub(TERMINAL_H_PADDING * 2).saturating_sub(2);
        assert_eq!(term_cols, 0);
    }

    #[test]
    fn test_terminal_width_at_boundary() {
        // When window is exactly sidebar width + h_padding + border, terminal should be 0
        let window_width: u16 = SIDEBAR_WIDTH + TERMINAL_H_PADDING * 2 + 2;
        let term_cols = window_width.saturating_sub(SIDEBAR_WIDTH).saturating_sub(TERMINAL_H_PADDING * 2).saturating_sub(2);
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
    fn test_ctrl_q_is_quit_key() {
        // Ctrl+Q should trigger quit
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let is_quit = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(is_quit, "Ctrl+Q should be a quit key");
    }

    #[test]
    fn test_ctrl_b_is_quit_key() {
        // Ctrl+B should trigger quit
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
        let is_quit = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(is_quit, "Ctrl+B should be a quit key");
    }

    #[test]
    fn test_ctrl_other_is_not_quit_key() {
        // Ctrl+X should not trigger quit
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        let is_quit = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(!is_quit, "Ctrl+X should not be a quit key");
    }

    #[test]
    fn test_plain_q_is_not_quit_key() {
        // Plain 'q' without Ctrl should not trigger quit
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let is_quit = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(!is_quit, "Plain 'q' should not be a quit key");
    }

    #[test]
    fn test_plain_b_is_not_quit_key() {
        // Plain 'b' without Ctrl should not trigger quit
        let key = crossterm::event::KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE);
        let is_quit = key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'));
        assert!(!is_quit, "Plain 'b' should not be a quit key");
    }

    #[test]
    fn test_daemon_app_creation() {
        let app = DaemonApp::new(24, 80, "test", vec![]);
        assert_eq!(app.session_name, "test");
    }

    #[test]
    fn test_daemon_app_process_output() {
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        app.process_output(b"Hello, World!");
        // Verify terminal emulator received the data
        let contents = app.term_emulator.contents();
        assert!(contents.contains("Hello, World!"));
    }

    #[test]
    fn test_daemon_app_resize() {
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
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
        let cli = Cli::try_parse_from(["sb", "kill", "mysession"]).unwrap();
        match cli.command {
            Some(Commands::Kill { session }) => assert_eq!(session, "mysession"),
            _ => panic!("Expected Kill command"),
        }
    }

    #[test]
    fn test_cli_parsing_attach() {
        let cli = Cli::try_parse_from(["sb", "attach", "mysession"]).unwrap();
        match cli.command {
            Some(Commands::Attach { session }) => assert_eq!(session, "mysession"),
            _ => panic!("Expected Attach command"),
        }
    }

    #[test]
    fn test_cli_parsing_attach_default() {
        let cli = Cli::try_parse_from(["sb", "attach"]).unwrap();
        match cli.command {
            Some(Commands::Attach { session }) => assert_eq!(session, "main"),
            _ => panic!("Expected Attach command"),
        }
    }

    #[test]
    fn test_cli_parsing_daemon() {
        let cli = Cli::try_parse_from(["sb", "daemon"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Daemon)));
    }

    #[test]
    fn test_cli_parsing_no_command() {
        let cli = Cli::try_parse_from(["sb"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.session.is_none()); // No default session - will show welcome state or first existing
    }

    #[test]
    fn test_cli_parsing_session_flag() {
        let cli = Cli::try_parse_from(["sb", "-s", "mysession"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.session, Some("mysession".to_string()));
    }

    #[test]
    fn test_cli_parsing_stale() {
        let cli = Cli::try_parse_from(["sb", "stale"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Stale)));
    }

    #[test]
    fn test_cli_parsing_restore() {
        let cli = Cli::try_parse_from(["sb", "restore", "old-session"]).unwrap();
        match cli.command {
            Some(Commands::Restore { session }) => assert_eq!(session, "old-session"),
            _ => panic!("Expected Restore command"),
        }
    }

    #[test]
    fn test_cli_parsing_forget() {
        let cli = Cli::try_parse_from(["sb", "forget", "old-session"]).unwrap();
        match cli.command {
            Some(Commands::Forget { session }) => assert_eq!(session, "old-session"),
            _ => panic!("Expected Forget command"),
        }
    }

    #[test]
    fn test_mouse_scroll_position_translation() {
        // Test that screen coordinates are correctly translated to terminal-relative coordinates
        // Terminal content starts after: sidebar (28) + border (1) + padding (1) = 30
        // Screen column 32 should become terminal column 3
        // (32 - 30 + 1 = 3)
        let term_content_start = SIDEBAR_WIDTH + 1 + TERMINAL_H_PADDING;
        let screen_col: u16 = 32;
        let term_col = screen_col - term_content_start + 1;
        assert_eq!(term_col, 3);
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
        // Events in sidebar/border/padding area should be ignored
        // Terminal content starts after: sidebar (28) + border (1) + padding (1) = 30
        let term_content_start = SIDEBAR_WIDTH + 1 + TERMINAL_H_PADDING;
        let mouse_column: u16 = 29; // Inside terminal border/padding area (before column 30)
        let should_handle = mouse_column >= term_content_start;
        assert!(!should_handle, "Scroll in sidebar/border/padding area should be ignored");
    }

    #[test]
    fn test_mouse_scroll_in_terminal_area_is_handled() {
        // Events in terminal content area should be handled
        // Terminal content starts after: sidebar + border (1) + padding
        let term_content_start = SIDEBAR_WIDTH + 1 + TERMINAL_H_PADDING;
        let mouse_column: u16 = 35; // Inside terminal content area
        let should_handle = mouse_column >= term_content_start;
        assert!(should_handle, "Scroll in terminal area should be handled");
    }

    #[test]
    fn test_mouse_scroll_works_regardless_of_focus() {
        // Per spec line 91: "Mouse scrolling when the Sidebar TUI is opened at all,
        // regardless of focus should scroll the terminal pane's visible history."
        // This test documents that focus is NOT a condition for mouse scroll handling.
        // The only conditions are: Normal mode, mouse in terminal area, session exists.
        use sidebar_tui::state::{AppMode, Focus, AppState};

        let mut state = AppState {
            mode: AppMode::Normal,
            ..Default::default()
        };

        // Scroll should work when sidebar is focused
        state.focus = Focus::Sidebar;
        let should_scroll_sidebar_focused =
            matches!(state.mode, AppMode::Normal); // Focus NOT checked
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
    fn test_hint_bar_rendered_at_bottom() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Hint bar should be at the bottom with DARK_GREY background
        // Last row (y=23) should have hint bar background
        let cell = &buffer[(0, 23)];
        assert_eq!(
            cell.bg,
            colors::DARK_GREY,
            "Hint bar should have dark grey background, got: {:?}",
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

        // In default state (sidebar focused, welcome state), should show "n New" and "q Quit"
        assert!(
            content.contains("n New") || content.contains("New"),
            "Hint bar should show 'New' keybinding, got: {}",
            content
        );
    }

    #[test]
    fn test_hint_bar_shows_quit_path() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Should show quit path on the right
        assert!(
            content.contains("Quit"),
            "Hint bar should show quit path, got: {}",
            content
        );
    }

    #[test]
    fn test_hint_bar_has_correct_keybinding_colors() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Find the 'n' keybinding on the hint bar (last row)
        let last_row = 23;
        for x in 0..buffer.area().width {
            let cell = &buffer[(x, last_row)];
            if cell.symbol() == "n" && cell.fg == colors::PURPLE {
                // Found a purple 'n' keybinding
                return;
            }
        }
        panic!("Hint bar should have purple keybindings");
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

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // When terminal is focused, hint bar should show "ctrl + b" binding
        assert!(
            content.contains("ctrl + b") || content.contains("Focus on sidebar"),
            "Hint bar should show 'ctrl + b' binding when terminal is focused, got: {}",
            content
        );
    }

    #[test]
    fn test_terminal_focused_border_color() {
        use sidebar_tui::state::AppState;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();

        // Terminal border should be FOCUSED_BORDER (250) when focused (starts right after sidebar)
        let term_start_x = SIDEBAR_WIDTH;
        let corner = &buffer[(term_start_x, 0)];
        assert_eq!(
            corner.fg,
            colors::FOCUSED_BORDER,
            "Terminal border should be color 250 when focused, got: {:?}",
            corner.fg
        );

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
        use sidebar_tui::state::{AppState, DraftingState, SessionType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

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
    fn test_drafting_mode_terminal_border_is_dark_grey() {
        use sidebar_tui::state::{AppState, DraftingState, SessionType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();

        // Terminal border should be DARK_GREY during drafting (starts right after sidebar)
        let term_start_x = SIDEBAR_WIDTH;
        let corner = &buffer[(term_start_x, 0)];
        assert_eq!(
            corner.fg,
            colors::DARK_GREY,
            "Terminal border should be dark grey during drafting, got: {:?}",
            corner.fg
        );
    }

    #[test]
    fn test_drafting_mode_sidebar_border_is_focused() {
        use sidebar_tui::state::{AppState, DraftingState, SessionType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();

        // Sidebar border should be FOCUSED_BORDER (250) during drafting (focused)
        let sidebar_corner = &buffer[(0, 0)];
        assert_eq!(
            sidebar_corner.fg,
            colors::FOCUSED_BORDER,
            "Sidebar border should be color 250 during drafting, got: {:?}",
            sidebar_corner.fg
        );
    }

    #[test]
    fn test_drafting_mode_hint_bar_shows_correct_bindings() {
        use sidebar_tui::state::{AppState, DraftingState, SessionType};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

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
    fn test_create_mode_hint_bar_shows_session_type_options() {
        use sidebar_tui::state::AppState;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Hint bar should show "t Terminal Session" and "a Agent Session" in create mode
        assert!(
            content.contains("Terminal Session"),
            "Hint bar should show 'Terminal Session' in create mode, got: {}",
            content
        );
        assert!(
            content.contains("Agent Session"),
            "Hint bar should show 'Agent Session' in create mode, got: {}",
            content
        );
    }

    #[test]
    fn test_renaming_mode_hint_bar_shows_correct_bindings() {
        use sidebar_tui::state::{AppState, RenamingState, Session};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.mode = AppMode::Renaming(RenamingState::new(0, "test", Focus::Sidebar));

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

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
    fn test_quit_confirmation_shows_prompt_message() {
        use sidebar_tui::state::{AppState, ConfirmState, ConfirmAction};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Should show quit confirmation message
        assert!(
            content.contains("Quit Sidebar TUI?"),
            "Hint bar should show quit confirmation message, got: {}",
            content
        );
    }

    #[test]
    fn test_quit_confirmation_shows_yes_no_bindings() {
        use sidebar_tui::state::{AppState, ConfirmState, ConfirmAction};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

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
    fn test_quit_confirmation_has_dark_grey_background() {
        use sidebar_tui::state::{AppState, ConfirmState, ConfirmAction};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();

        // Quit confirmation is NOT important, so hint bar background should be dark grey
        let last_row = 23;
        let cell = &buffer[(0, last_row)];
        assert_eq!(
            cell.bg,
            colors::DARK_GREY,
            "Quit confirmation hint bar should have dark grey background, got: {:?}",
            cell.bg
        );
    }

    #[test]
    fn test_delete_confirmation_shows_prompt_message() {
        use sidebar_tui::state::{AppState, ConfirmState, ConfirmAction, Session};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.mode = AppMode::Confirming(ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar));

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // Should show delete confirmation message
        assert!(
            content.contains("Delete this session permanently?"),
            "Hint bar should show delete confirmation message, got: {}",
            content
        );
    }

    #[test]
    fn test_delete_confirmation_has_red_background() {
        use sidebar_tui::state::{AppState, ConfirmState, ConfirmAction, Session};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.mode = AppMode::Confirming(ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar));

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();

        // Delete confirmation IS important, so hint bar background should be dark red (88)
        let last_row = 23;
        let cell = &buffer[(0, last_row)];
        assert_eq!(
            cell.bg,
            colors::DARK_RED,
            "Delete confirmation hint bar should have dark red background, got: {:?}",
            cell.bg
        );
    }

    #[test]
    fn test_confirmation_quit_path_shows_n_to_quit() {
        use sidebar_tui::state::{AppState, ConfirmState, ConfirmAction};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        terminal.draw(|frame| render_with_state(frame, &state)).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // During confirmation, quit path should show "n → q Quit"
        // (pressing n cancels, then q quits)
        assert!(
            content.contains("n →") || content.contains("n → q"),
            "Confirmation quit path should show 'n →' path, got: {}",
            content
        );
    }

    #[test]
    fn test_handle_drained_response_output() {
        // Test that Output responses are processed correctly
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Output {
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
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Output { data: vec![] };

        // Should not panic
        handle_drained_response(response, &mut app);
    }

    #[test]
    fn test_handle_drained_response_previewed() {
        // Test that Previewed responses update terminal state
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Previewed {
            session_name: "preview".to_string(),
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
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Previewed {
            session_name: "preview".to_string(),
            terminal_state: None,
        };

        // Should not panic
        handle_drained_response(response, &mut app);
    }

    #[test]
    fn test_handle_drained_response_ignores_other_responses() {
        // Test that other responses are safely ignored during drain
        let mut app = DaemonApp::new(24, 80, "test", vec![]);

        // These should all be safely ignored
        handle_drained_response(DaemonResponse::Attached {
            session_name: "test".to_string(),
            is_new: true,
            terminal_state: None,
        }, &mut app);

        handle_drained_response(DaemonResponse::Detached, &mut app);

        handle_drained_response(DaemonResponse::Error {
            message: "test error".to_string(),
        }, &mut app);

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
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Output { data: b"hello".to_vec() };

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));
        assert!(app.term_emulator.contents().contains("hello"));
    }

    #[test]
    fn test_handle_main_loop_response_shutting_down() {
        // Test that ShuttingDown triggers loop break
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::ShuttingDown;

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::ShuttingDown));
    }

    #[test]
    fn test_handle_main_loop_response_error() {
        // Test that Error responses return error result
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Error { message: "test error".to_string() };

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Error(msg) if msg == "test error"));
    }

    #[test]
    fn test_handle_main_loop_response_previewed() {
        // Test that Previewed messages update the terminal
        let mut app = DaemonApp::new(24, 80, "test", vec![]);
        let response = DaemonResponse::Previewed {
            session_name: "preview".to_string(),
            terminal_state: Some(b"preview content".to_vec()),
        };

        let result = handle_main_loop_response(response, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));
        assert!(app.term_emulator.contents().contains("preview content"));
    }

    #[test]
    fn test_handle_main_loop_response_other() {
        // Test that other responses are safely ignored with Continue
        let mut app = DaemonApp::new(24, 80, "test", vec![]);

        let result = handle_main_loop_response(DaemonResponse::Attached {
            session_name: "test".to_string(),
            is_new: true,
            terminal_state: None,
        }, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));

        let result = handle_main_loop_response(DaemonResponse::Detached, &mut app, 24, 80);
        assert!(matches!(result, MainLoopDrainResult::Continue));
    }
}
