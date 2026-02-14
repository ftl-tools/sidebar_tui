use std::env;
use std::io::{self, Write as IoWrite};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::{Context, bail};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};

use sidebar_tui::daemon::{
    self, ClientMessage, DaemonClient, DaemonResponse, get_socket_path,
    ensure_runtime_dir, decode_message, encode_message,
};
use sidebar_tui::input::key_to_bytes;
use sidebar_tui::terminal::Terminal;

/// Sidebar TUI - A terminal session manager
#[derive(Parser, Debug)]
#[command(name = "sb")]
#[command(about = "A terminal session manager with session persistence", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Session name to attach to (default: "main")
    #[arg(short, long, default_value = "main")]
    session: String,
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
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::List) => cmd_list(),
        Some(Commands::Kill { session }) => cmd_kill(&session),
        Some(Commands::Attach { session }) => cmd_attach(&session),
        Some(Commands::Daemon) => cmd_daemon(),
        None => cmd_attach(&cli.session),
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

/// Start the daemon process (runs in foreground).
fn cmd_daemon() -> Result<()> {
    let daemon = daemon::Daemon::new()?;
    println!("Starting daemon at {:?}", daemon.socket_path());
    daemon.run()
}

/// Attach to a session (or create if it doesn't exist).
fn cmd_attach(session_name: &str) -> Result<()> {
    // Ensure daemon is running
    ensure_daemon_running()?;

    // Connect to daemon
    let socket_path = get_socket_path();
    let mut stream = UnixStream::connect(&socket_path)
        .context("Failed to connect to daemon")?;

    // Set read timeout for non-blocking reads
    stream.set_read_timeout(Some(Duration::from_millis(1)))
        .context("Failed to set read timeout")?;

    // Initialize TUI
    let mut ratatui_term = ratatui::init();
    let result = run_attached(&mut ratatui_term, &mut stream, session_name);
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

/// Application state for daemon-connected mode.
struct DaemonApp {
    /// Terminal emulator for parsing PTY output
    term_emulator: Terminal,
    /// Current session name (kept for future use with multiple sessions)
    #[allow(dead_code)]
    session_name: String,
}

impl DaemonApp {
    fn new(rows: u16, cols: u16, session_name: &str) -> Self {
        Self {
            term_emulator: Terminal::new(rows, cols),
            session_name: session_name.to_string(),
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

/// Run the TUI attached to a daemon session.
fn run_attached(
    ratatui_term: &mut DefaultTerminal,
    stream: &mut UnixStream,
    session_name: &str,
) -> Result<()> {
    // Get initial terminal size
    let size = ratatui_term.size()?;
    let term_cols = size.width.saturating_sub(SIDEBAR_WIDTH);
    let term_rows = size.height;

    // Get current working directory
    let cwd = env::current_dir().ok();

    // Send attach message
    let attach_msg = ClientMessage::Attach {
        session_name: session_name.to_string(),
        rows: term_rows,
        cols: term_cols,
        cwd,
    };
    let encoded = encode_message(&attach_msg)?;
    stream.write_all(&encoded)?;
    stream.flush()?;

    // Read attach response
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let response: DaemonResponse = decode_message(stream)
        .context("Failed to read attach response")?;

    let terminal_state = match response {
        DaemonResponse::Attached { session_name: _, is_new: _, terminal_state } => {
            terminal_state
        }
        DaemonResponse::Error { message } => {
            bail!("Failed to attach: {}", message);
        }
        other => {
            bail!("Unexpected response: {:?}", other);
        }
    };

    // Create app
    let mut app = DaemonApp::new(term_rows, term_cols, session_name);

    // Restore terminal state if reattaching
    if let Some(state_bytes) = terminal_state {
        app.process_output(&state_bytes);
    }

    let mut last_size = (size.width, size.height);

    // Set stream back to non-blocking for the main loop
    stream.set_read_timeout(Some(Duration::from_millis(1)))?;

    loop {
        // Try to read output from daemon
        match try_read_response(stream) {
            Ok(Some(DaemonResponse::Output { data })) => {
                if !data.is_empty() {
                    app.process_output(&data);
                }
            }
            Ok(Some(DaemonResponse::ShuttingDown)) => {
                break;
            }
            Ok(Some(DaemonResponse::Error { message })) => {
                bail!("Daemon error: {}", message);
            }
            Ok(Some(_)) => {
                // Other responses, ignore
            }
            Ok(None) => {
                // No data available
            }
            Err(e) => {
                // Check if it's a non-blocking "no data" error
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut {
                    // No data, continue
                } else {
                    // Real error or disconnect
                    bail!("Connection error: {}", e);
                }
            }
        }

        // Render the UI
        ratatui_term.draw(|frame| render_daemon_app(frame, &app))?;

        // Handle input events
        if event::poll(Duration::from_millis(16)).context("event poll failed")? {
            match event::read().context("event read failed")? {
                Event::Key(key) => {
                    // Check for Ctrl+Q or Ctrl+B to quit (detach)
                    if key.modifiers == KeyModifiers::CONTROL
                        && (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('b'))
                    {
                        // Send detach message
                        let detach_msg = ClientMessage::Detach;
                        let encoded = encode_message(&detach_msg)?;
                        stream.write_all(&encoded)?;
                        stream.flush()?;
                        break;
                    }

                    // Forward other keys to daemon
                    let bytes = key_to_bytes(&key);
                    if !bytes.is_empty() {
                        let input_msg = ClientMessage::Input { data: bytes };
                        let encoded = encode_message(&input_msg)?;
                        stream.write_all(&encoded)?;
                        stream.flush()?;
                    }
                }
                Event::Resize(width, height) => {
                    if (width, height) != last_size {
                        last_size = (width, height);
                        let term_cols = width.saturating_sub(SIDEBAR_WIDTH);
                        app.resize(height, term_cols);

                        // Send resize to daemon
                        let resize_msg = ClientMessage::Resize {
                            rows: height,
                            cols: term_cols,
                        };
                        let encoded = encode_message(&resize_msg)?;
                        stream.write_all(&encoded)?;
                        stream.flush()?;
                    }
                }
                Event::Mouse(_) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {
                    // Not handled yet
                }
            }
        }
    }

    Ok(())
}

/// Try to read a response from the stream without blocking.
fn try_read_response(stream: &mut UnixStream) -> io::Result<Option<DaemonResponse>> {
    match decode_message::<DaemonResponse>(stream) {
        Ok(response) => Ok(Some(response)),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(None),
        Err(e) => Err(e),
    }
}

/// Sidebar width in characters
pub const SIDEBAR_WIDTH: u16 = 20;

/// Render the application UI with daemon-connected terminal emulator.
fn render_daemon_app(frame: &mut Frame, app: &DaemonApp) {
    // Create horizontal layout: sidebar (20 chars) + terminal view (rest)
    let chunks = Layout::horizontal([
        Constraint::Length(SIDEBAR_WIDTH),
        Constraint::Fill(1),
    ])
    .split(frame.area());

    render_sidebar(frame, chunks[0]);
    render_terminal_emulator(frame, chunks[1], &app.term_emulator);
}

/// Render the static UI layout (for tests without PTY).
pub fn render(frame: &mut Frame) {
    // Create horizontal layout: sidebar (20 chars) + terminal view (rest)
    let chunks = Layout::horizontal([
        Constraint::Length(SIDEBAR_WIDTH),
        Constraint::Fill(1),
    ])
    .split(frame.area());

    render_sidebar(frame, chunks[0]);
    render_terminal_view(frame, chunks[1]);
}

fn render_sidebar(frame: &mut Frame, area: Rect) {
    // Split sidebar into header row and body
    let sidebar_chunks = Layout::vertical([
        Constraint::Length(1), // Header row
        Constraint::Fill(1),   // Body
    ])
    .split(area);

    // Header: blue background, black text, centered
    let header = Paragraph::new("Sidebar TUI")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Black).bg(Color::Blue));
    frame.render_widget(header, sidebar_chunks[0]);

    // Body: lighter background (dark gray), empty for now
    let body = Paragraph::new("")
        .style(Style::default().bg(Color::DarkGray));
    frame.render_widget(body, sidebar_chunks[1]);
}

fn render_terminal_view(frame: &mut Frame, area: Rect) {
    // Terminal view - placeholder for static tests
    let terminal_placeholder = Paragraph::new("Terminal view (press Ctrl+Q or Ctrl+B to quit)")
        .style(Style::default().bg(Color::Black).fg(Color::White));
    frame.render_widget(terminal_placeholder, area);
}

fn render_terminal_emulator(frame: &mut Frame, area: Rect, term: &Terminal) {
    // Render the terminal emulator content with cursor
    if let Some((cursor_x, cursor_y)) = term.render_with_cursor(frame, area) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_sidebar_header_shows_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Sidebar TUI"),
            "Should contain 'Sidebar TUI', got: {}",
            content
        );
    }

    #[test]
    fn test_sidebar_header_has_blue_background() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Check that the header row (first row, first 20 columns) has blue background
        for x in 0..SIDEBAR_WIDTH {
            let cell = &buffer[(x, 0)];
            assert_eq!(
                cell.bg,
                Color::Blue,
                "Sidebar header at ({}, 0) should have blue background, got: {:?}",
                x,
                cell.bg
            );
        }
    }

    #[test]
    fn test_sidebar_header_has_black_text() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Find the 'S' in "Sidebar TUI" and check its foreground color
        let header_text = "Sidebar TUI";
        let start_x = (SIDEBAR_WIDTH - header_text.len() as u16) / 2;

        for (i, _) in header_text.chars().enumerate() {
            let cell = &buffer[(start_x + i as u16, 0)];
            assert_eq!(
                cell.fg,
                Color::Black,
                "Sidebar header text at ({}, 0) should have black foreground, got: {:?}",
                start_x + i as u16,
                cell.fg
            );
        }
    }

    #[test]
    fn test_sidebar_header_is_centered() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Extract first row within sidebar width
        let mut header_content = String::new();
        for x in 0..SIDEBAR_WIDTH {
            let cell = &buffer[(x, 0)];
            header_content.push_str(cell.symbol());
        }

        // "Sidebar TUI" is 11 chars, sidebar is 20, so 9 spaces total
        // Ratatui centers with floor(remaining/2) on left, so we get 4 left + 5 right
        // Verify the text is roughly centered (within 1 char tolerance)
        let trimmed = header_content.trim();
        assert_eq!(
            trimmed, "Sidebar TUI",
            "Header should contain 'Sidebar TUI'"
        );

        // Count leading spaces
        let leading_spaces = header_content.len() - header_content.trim_start().len();
        // Should be approximately centered: 9 total spaces / 2 = 4-5 on each side
        assert!(
            leading_spaces >= 4 && leading_spaces <= 5,
            "Header should have 4-5 leading spaces for centering, got: {}",
            leading_spaces
        );
    }

    #[test]
    fn test_sidebar_body_has_lighter_background() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Check that the sidebar body (row 1+, first 20 columns) has dark gray background
        for y in 1..24u16 {
            for x in 0..SIDEBAR_WIDTH {
                let cell = &buffer[(x, y)];
                assert_eq!(
                    cell.bg,
                    Color::DarkGray,
                    "Sidebar body at ({}, {}) should have dark gray background, got: {:?}",
                    x,
                    y,
                    cell.bg
                );
            }
        }
    }

    #[test]
    fn test_sidebar_is_20_chars_wide() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // The sidebar should be exactly 20 characters wide
        // Check that column 19 (last sidebar column) has sidebar styling
        // and column 20 (first terminal column) has terminal styling
        assert_eq!(
            buffer[(19, 0)].bg,
            Color::Blue,
            "Column 19 (last sidebar header) should be blue"
        );
        assert_eq!(
            buffer[(20, 0)].bg,
            Color::Black,
            "Column 20 (first terminal column) should be black"
        );
    }

    #[test]
    fn test_terminal_view_fills_rest() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Terminal view should start at column 20 and have black background
        for x in 20..80u16 {
            let cell = &buffer[(x, 0)];
            assert_eq!(
                cell.bg,
                Color::Black,
                "Terminal view at ({}, 0) should have black background, got: {:?}",
                x,
                cell.bg
            );
        }
    }

    #[test]
    fn test_terminal_view_shows_placeholder() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Ctrl+Q") && content.contains("Ctrl+B"),
            "Terminal view should contain both 'Ctrl+Q' and 'Ctrl+B', got: {}",
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

        // The sidebar title should still appear
        assert!(
            content.contains("Sidebar TUI"),
            "Should contain 'Sidebar TUI', got: {}",
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
    fn test_terminal_width_excludes_sidebar() {
        // When window is 100 wide, terminal should be 100 - SIDEBAR_WIDTH = 80
        let window_width: u16 = 100;
        let term_cols = window_width.saturating_sub(SIDEBAR_WIDTH);
        assert_eq!(term_cols, 80);
    }

    #[test]
    fn test_terminal_width_handles_small_window() {
        // When window is smaller than sidebar, terminal width should be 0 (saturating sub)
        let window_width: u16 = 15;
        let term_cols = window_width.saturating_sub(SIDEBAR_WIDTH);
        assert_eq!(term_cols, 0);
    }

    #[test]
    fn test_terminal_width_at_boundary() {
        // When window is exactly sidebar width, terminal should be 0
        let window_width: u16 = SIDEBAR_WIDTH;
        let term_cols = window_width.saturating_sub(SIDEBAR_WIDTH);
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
        let app = DaemonApp::new(24, 80, "test");
        assert_eq!(app.session_name, "test");
    }

    #[test]
    fn test_daemon_app_process_output() {
        let mut app = DaemonApp::new(24, 80, "test");
        app.process_output(b"Hello, World!");
        // Verify terminal emulator received the data
        let contents = app.term_emulator.contents();
        assert!(contents.contains("Hello, World!"));
    }

    #[test]
    fn test_daemon_app_resize() {
        let mut app = DaemonApp::new(24, 80, "test");
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
        assert_eq!(cli.session, "main");
    }

    #[test]
    fn test_cli_parsing_session_flag() {
        let cli = Cli::try_parse_from(["sb", "-s", "mysession"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.session, "mysession");
    }
}
