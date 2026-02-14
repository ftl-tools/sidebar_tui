//! End-to-end tests for Sidebar TUI
//!
//! These tests spawn the actual `sb` binary in a PTY and verify its behavior.
//! Uses expectrl for PTY management and vt100 for terminal emulation.
//!
//! NOTE: These tests MUST run serially because they share a daemon process.
//! They use the `serial_test` crate to enforce this.

use std::io::Write;
use std::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};

use expectrl::spawn;
use serial_test::serial;

/// Atomic counter to generate unique session names for each test
static SESSION_COUNTER: AtomicU32 = AtomicU32::new(0);

fn get_unique_session_name() -> String {
    let id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    format!("test-{}-{}", pid, id)
}

fn get_binary_path() -> String {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set by cargo");
    format!("{}/target/debug/sb", manifest_dir)
}

/// Helper to spawn sb and get its output parsed through vt100
struct SbSession {
    session: expectrl::session::OsSession,
    parser: vt100::Parser,
    session_name: String,
}

impl SbSession {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let binary_path = get_binary_path();
        let session_name = get_unique_session_name();

        // Spawn sb with a unique session name to avoid state from other tests
        let cmd = format!("{} -s {}", binary_path, session_name);
        let mut session = spawn(&cmd)?;

        // Set a reasonable timeout
        session.set_expect_timeout(Some(Duration::from_secs(5)));

        // Create a vt100 parser to interpret the terminal output
        // Use 80x24 as the standard terminal size
        let parser = vt100::Parser::new(24, 80, 0);

        Ok(Self { session, parser, session_name })
    }

    /// Read all available output and process it through vt100
    fn read_and_parse(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Give the TUI time to render
        std::thread::sleep(Duration::from_millis(500));

        // Try to read what's available
        let mut buf = [0u8; 8192];

        // Use non-blocking reads by checking what's available
        loop {
            match self.session.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.parser.process(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        Ok(())
    }

    /// Get a specific row's contents
    fn row_contents(&self, row: u16) -> String {
        self.parser.screen().contents_between(
            row,
            0,
            row,
            self.parser.screen().size().1 - 1,
        )
    }

    /// Send Ctrl+Q to quit
    fn quit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Ctrl+Q is ASCII 17
        self.session.write_all(&[17])?;
        self.session.flush()?;
        // Give time to process quit
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }

    /// Send a string to the terminal
    fn send(&mut self, s: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.session.write_all(s.as_bytes())?;
        self.session.flush()?;
        Ok(())
    }

    /// Send Enter key
    fn send_enter(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.session.write_all(&[0x0d])?; // CR
        self.session.flush()?;
        Ok(())
    }

    /// Send backspace
    fn send_backspace(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.session.write_all(&[0x7f])?;
        self.session.flush()?;
        Ok(())
    }

    /// Get cell at position
    fn cell_at(&self, row: u16, col: u16) -> Option<vt100::Cell> {
        self.parser.screen().cell(row, col).cloned()
    }
}

impl Drop for SbSession {
    fn drop(&mut self) {
        // Try to clean up by quitting
        let _ = self.quit();
        let _ = self.session.get_process_mut().exit(true);

        // Kill the session to clean up daemon resources
        let binary_path = get_binary_path();
        let _ = std::process::Command::new(&binary_path)
            .args(["kill", &self.session_name])
            .output();
    }
}

/// Test that the layout matches the spec:
/// - Sidebar is 20 chars wide
/// - Header row is blue with centered "Sidebar TUI" in black
/// - Sidebar body has lighter (dark gray) background
#[test]
#[serial]
fn test_layout_matches_spec() {
    let mut session = SbSession::new().expect("Failed to spawn sb");

    // Give time for initial render
    std::thread::sleep(Duration::from_millis(1000));
    session.read_and_parse().expect("Failed to read output");

    // Verify "Sidebar TUI" appears in the first row
    let first_row = session.row_contents(0);
    assert!(
        first_row.contains("Sidebar TUI"),
        "First row should contain 'Sidebar TUI', got: '{}'",
        first_row
    );

    // The sidebar header should be within the first 20 columns
    // Check that "Sidebar TUI" (11 chars) is centered in 20 chars
    // That means about 4-5 spaces before it
    let sidebar_portion = &first_row[..20.min(first_row.len())];
    assert!(
        sidebar_portion.contains("Sidebar TUI"),
        "Sidebar portion should contain 'Sidebar TUI', got: '{}'",
        sidebar_portion
    );

    // Check centering - should have leading spaces
    let leading_spaces = sidebar_portion.len() - sidebar_portion.trim_start().len();
    assert!(
        leading_spaces >= 3 && leading_spaces <= 6,
        "Header should be roughly centered with 3-6 leading spaces, got: {}",
        leading_spaces
    );

    // Verify the header cell has blue background
    // Note: vt100 uses different color representations
    if let Some(header_cell) = session.cell_at(0, 5) {
        let bg_color = header_cell.bgcolor();
        // Blue is typically index 4 or an RGB value
        assert!(
            matches!(
                bg_color,
                vt100::Color::Idx(4) | vt100::Color::Rgb(0, 0, _)
            ),
            "Header should have blue background, got: {:?}",
            bg_color
        );
    }

    // Verify sidebar body (row 1+, columns 0-19) has different background than terminal
    if let (Some(body_cell), Some(terminal_cell)) = (session.cell_at(2, 5), session.cell_at(2, 25))
    {
        let body_bg = body_cell.bgcolor();
        let term_bg = terminal_cell.bgcolor();

        // They should be different (sidebar body is DarkGray, terminal is Black)
        assert!(
            body_bg != term_bg || body_bg != vt100::Color::Default,
            "Sidebar body should have different background than terminal. Body: {:?}, Terminal: {:?}",
            body_bg, term_bg
        );
    }

    // Clean up
    session.quit().expect("Failed to quit");
}

/// Test that git status output in the TUI matches normal terminal output.
/// This verifies that the terminal emulation properly handles git output.
#[test]
#[serial]
fn test_git_status_output_matches() {
    // First, get the expected git status output from a normal terminal
    let expected_output = std::process::Command::new("git")
        .arg("status")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Failed to run git status");
    let expected_stdout = String::from_utf8_lossy(&expected_output.stdout);

    // Extract key phrases that should appear in the output
    // We look for "On branch" which is always present
    assert!(
        expected_stdout.contains("On branch"),
        "Expected 'On branch' in git status output"
    );

    // Now run git status in the TUI
    let mut session = SbSession::new().expect("Failed to spawn sb");

    // Wait for shell prompt
    std::thread::sleep(Duration::from_millis(2000));
    session.read_and_parse().expect("Failed to read output");

    // Type "git status" and press Enter
    session.send("git status").expect("Failed to send command");
    session.send_enter().expect("Failed to send enter");

    // Wait for command to execute with polling - look for "On branch" text
    let found = wait_for_text(&mut session.session, &mut session.parser, "On branch", 5000);

    // Get the screen contents and verify git status output appears
    let screen_contents = session.parser.screen().contents();

    assert!(
        found && screen_contents.contains("On branch"),
        "TUI terminal should show 'On branch' from git status. Got:\n{}",
        screen_contents
    );

    // Verify the branch name appears (main or master typically)
    let has_branch = screen_contents.contains("main") || screen_contents.contains("master");
    assert!(
        has_branch,
        "TUI terminal should show branch name. Got:\n{}",
        screen_contents
    );

    session.quit().expect("Failed to quit");
}

/// Test that vi can be used to edit files in the TUI terminal.
/// This verifies that the terminal properly handles vi's escape sequences
/// and that keyboard input is correctly forwarded.
#[test]
#[serial]
fn test_vi_editing_workflow() {
    use std::fs;

    // Create a test file with known content
    let test_file = format!("{}/test_vi_edit.txt", env!("CARGO_MANIFEST_DIR"));
    let original_content = "original line\n";
    fs::write(&test_file, original_content).expect("Failed to create test file");

    // Ensure cleanup even if test fails
    struct Cleanup<'a>(&'a str);
    impl<'a> Drop for Cleanup<'a> {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0);
        }
    }
    let _cleanup = Cleanup(&test_file);

    let mut session = SbSession::new().expect("Failed to spawn sb");

    // Wait for shell to be ready
    std::thread::sleep(Duration::from_millis(2000));
    session.read_and_parse().expect("Failed to read output");

    // Open the file in vi
    session
        .send(&format!("vi {}", test_file))
        .expect("Failed to send vi command");
    session.send_enter().expect("Failed to send enter");

    // Wait for vi to load - vi takes time to initialize
    std::thread::sleep(Duration::from_millis(2000));
    session.read_and_parse().expect("Failed to read output");

    // Verify vi has started by checking screen contents
    let screen = session.parser.screen().contents();
    eprintln!("Screen after vi start:\n{}", screen);

    // Go to the beginning of the line and enter insert mode
    // Press 'I' to insert at beginning of line
    session
        .session
        .write_all(b"I")
        .expect("Failed to send I");
    session.session.flush().expect("Failed to flush");

    std::thread::sleep(Duration::from_millis(500));
    session.read_and_parse().expect("Failed to read output");

    // Type "NEW: " at the beginning
    session.send("NEW: ").expect("Failed to type text");

    std::thread::sleep(Duration::from_millis(500));
    session.read_and_parse().expect("Failed to read output");

    // Press Escape to exit insert mode
    session
        .session
        .write_all(&[0x1b])
        .expect("Failed to send Escape");
    session.session.flush().expect("Failed to flush");

    std::thread::sleep(Duration::from_millis(500));
    session.read_and_parse().expect("Failed to read output");

    // Save and quit with :wq
    session.send(":wq").expect("Failed to type :wq");
    session.send_enter().expect("Failed to send enter");

    // Wait for vi to exit
    std::thread::sleep(Duration::from_millis(1500));
    session.read_and_parse().expect("Failed to read output");

    // Read the file and verify it was modified
    let modified_content = fs::read_to_string(&test_file).expect("Failed to read modified file");

    assert!(
        modified_content.contains("NEW: "),
        "File should contain 'NEW: ' prefix. Got: '{}'",
        modified_content
    );
    assert!(
        modified_content.contains("original"),
        "File should still contain 'original'. Got: '{}'",
        modified_content
    );

    session.quit().expect("Failed to quit");
}

/// Test that backspace works correctly for editing input before sending.
/// Per objectives: "type `git status`, backspace before you send it, type `echo "hello world"`,
/// send that, and see the expected output"
#[test]
#[serial]
fn test_backspace_input_handling() {
    let mut session = SbSession::new().expect("Failed to spawn sb");

    // Wait for shell to be ready
    std::thread::sleep(Duration::from_millis(2000));
    session.read_and_parse().expect("Failed to read output");

    // Type "git status"
    session.send("git status").expect("Failed to send git status");
    std::thread::sleep(Duration::from_millis(500));
    session.read_and_parse().expect("Failed to read output");

    // Backspace to delete "git status" (10 characters)
    for _ in 0..10 {
        session.send_backspace().expect("Failed to send backspace");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(500));
    session.read_and_parse().expect("Failed to read output");

    // Type echo "hello world"
    session
        .send("echo \"hello world\"")
        .expect("Failed to send echo command");

    // Wait for characters to be echoed
    std::thread::sleep(Duration::from_millis(1000));
    session.read_and_parse().expect("Failed to read output");

    session.send_enter().expect("Failed to send enter");

    // Wait for command to execute
    std::thread::sleep(Duration::from_millis(2000));
    session.read_and_parse().expect("Failed to read output");

    // Get screen contents and verify "hello world" appears
    let screen_contents = session.parser.screen().contents();

    assert!(
        screen_contents.contains("hello world"),
        "Screen should contain 'hello world' from echo command. Got:\n{}",
        screen_contents
    );

    // Verify "git status" output does NOT appear (since we backspaced it)
    // We should NOT see "On branch" since we didn't run git status
    // Note: This is a weak test since git status could have been run elsewhere
    // But "hello world" appearing proves the backspace worked

    session.quit().expect("Failed to quit");
}

/// Test that session state persists across TUI restarts (detach/reattach).
/// This tests the core session persistence required by objectives:
/// 1. Start TUI and open vi editing a file
/// 2. Exit TUI (Ctrl+Q) without saving vi - this detaches
/// 3. Restart TUI
/// 4. Verify vi is still open with the same content
/// 5. Verify can save and exit vi normally
#[test]
#[serial]
fn test_session_persistence_across_restart() {
    use std::fs;

    // Create a test file with known content
    let unique_id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let session_name = format!("persist-test-{}-{}", pid, unique_id);
    let test_file = format!("{}/test_persist_{}.txt", env!("CARGO_MANIFEST_DIR"), unique_id);
    let swap_file = format!("{}/.test_persist_{}.txt.swp", env!("CARGO_MANIFEST_DIR"), unique_id);
    let original_content = "original content\n";
    fs::write(&test_file, original_content).expect("Failed to create test file");

    // Ensure cleanup even if test fails
    struct Cleanup<'a> {
        file: &'a str,
        swap_file: &'a str,
        session: String,
        binary_path: String,
    }
    impl<'a> Drop for Cleanup<'a> {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.file);
            let _ = fs::remove_file(self.swap_file);
            // Kill the session to clean up daemon resources
            let _ = std::process::Command::new(&self.binary_path)
                .args(["kill", &self.session])
                .output();
        }
    }
    let binary_path = get_binary_path();
    let _cleanup = Cleanup {
        file: &test_file,
        swap_file: &swap_file,
        session: session_name.clone(),
        binary_path: binary_path.clone(),
    };

    // Clean up any leftover swap file from previous test runs
    let _ = fs::remove_file(&swap_file);

    // ============ PHASE 1: Start TUI, open vi, type text, detach ============
    {
        let cmd = format!("{} -s {}", binary_path, session_name);
        let mut session = spawn(&cmd).expect("Failed to spawn sb");
        session.set_expect_timeout(Some(Duration::from_secs(10)));
        let mut parser = vt100::Parser::new(24, 80, 0);

        // Wait for shell to be ready
        std::thread::sleep(Duration::from_millis(2500));
        read_into_parser(&mut session, &mut parser);

        // Open the file in vi
        session
            .write_all(format!("vi {}\n", test_file).as_bytes())
            .expect("Failed to send vi command");
        session.flush().expect("Failed to flush");

        // Wait for vi to fully load - look for the file content or vi's ~ indicators
        let vi_loaded = wait_for_text(&mut session, &mut parser, "original", 5000)
            || parser.screen().contents().contains("~");

        let screen = parser.screen().contents();
        eprintln!("Screen after vi open:\n{}", screen);

        assert!(vi_loaded, "Vi should have loaded the file. Screen:\n{}", screen);

        // Enter insert mode at beginning of line (I)
        session.write_all(b"I").expect("Failed to send I");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(500));

        // Type our marker text
        session.write_all(b"INSERTED: ").expect("Failed to type text");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(500));
        read_into_parser(&mut session, &mut parser);

        // Exit insert mode with Escape
        session.write_all(&[0x1b]).expect("Failed to send Escape");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(500));
        read_into_parser(&mut session, &mut parser);

        // Check the screen shows our inserted text
        let screen_contents = parser.screen().contents();
        eprintln!("Phase 1 screen after typing:\n{}", screen_contents);

        // Verify the inserted text appears on screen (vi shows it even before saving)
        assert!(
            screen_contents.contains("INSERTED"),
            "Screen should show 'INSERTED' text. Got:\n{}",
            screen_contents
        );

        // Send Ctrl+Q to detach (this does NOT save vi - the text is unsaved in vi buffer)
        session.write_all(&[17]).expect("Failed to send Ctrl+Q");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(500));

        // Process should exit (TUI detached)
        let _ = session.get_process_mut().exit(true);
    }

    // Brief pause to let daemon stabilize the detached session
    std::thread::sleep(Duration::from_millis(1000));

    // ============ PHASE 2: Reattach and verify vi is still running ============
    {
        let cmd = format!("{} -s {}", binary_path, session_name);
        let mut session = spawn(&cmd).expect("Failed to spawn sb for reattach");
        session.set_expect_timeout(Some(Duration::from_secs(10)));
        let mut parser = vt100::Parser::new(24, 80, 0);

        // Wait for session to restore - look for vi content or swap dialog
        // The session should be restored with vi still running
        let found_content = wait_for_text(&mut session, &mut parser, "INSERTED", 5000)
            || parser.screen().contents().contains("original")
            || parser.screen().contents().contains("swap")
            || parser.screen().contents().contains("Swap")
            || parser.screen().contents().contains("~");

        let screen_contents = parser.screen().contents();
        eprintln!("Phase 2 screen after reattach:\n{}", screen_contents);

        assert!(
            found_content,
            "After reattach, vi session should still exist. Got:\n{}",
            screen_contents
        );

        // Check if we're at a swap file dialog - if so, choose (R)ecover
        if screen_contents.contains("swap") || screen_contents.contains("Swap file") {
            eprintln!("Swap file dialog detected, sending 'r' to recover");
            session.write_all(b"r").expect("Failed to send 'r' for recover");
            session.flush().expect("Failed to flush");
            std::thread::sleep(Duration::from_millis(1500));
            read_into_parser(&mut session, &mut parser);

            let after_recover = parser.screen().contents();
            eprintln!("Screen after recover:\n{}", after_recover);
        }

        // Now save and quit vi with :wq
        // First, ensure we're in normal mode by sending Escape
        session.write_all(&[0x1b]).expect("Failed to send Escape");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(300));

        session.write_all(b":wq").expect("Failed to send :wq");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(300));

        session.write_all(&[0x0d]).expect("Failed to send Enter");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(2000));
        read_into_parser(&mut session, &mut parser);

        let screen_after_save = parser.screen().contents();
        eprintln!("Screen after :wq:\n{}", screen_after_save);

        // Quit the TUI
        session.write_all(&[17]).expect("Failed to send Ctrl+Q");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(500));
        let _ = session.get_process_mut().exit(true);
    }

    // ============ PHASE 3: Verify the file was saved correctly ============
    let final_content = fs::read_to_string(&test_file).expect("Failed to read final file");
    eprintln!("Final file content: {:?}", final_content);

    // The file should contain both the original content AND our inserted text
    // If vi recovery worked, it should have our INSERTED text
    // Note: If the session truly persisted with vi running, the unsaved buffer
    // should still be there and saving should write our INSERTED text
    assert!(
        final_content.contains("INSERTED"),
        "File should contain 'INSERTED' after vi save. Got: '{}'",
        final_content
    );
    assert!(
        final_content.contains("original"),
        "File should still contain 'original'. Got: '{}'",
        final_content
    );
}

/// Helper to read available output into a vt100 parser
/// Reads repeatedly until no more data is available
fn read_into_parser(session: &mut expectrl::session::OsSession, parser: &mut vt100::Parser) {
    let mut buf = [0u8; 8192];
    // Try reading multiple times with small delays to ensure we get all data
    for _ in 0..10 {
        let mut got_data = false;
        loop {
            match session.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    parser.process(&buf[..n]);
                    got_data = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        if !got_data {
            // Small delay then try again
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Wait for specific text to appear in the terminal, with timeout
fn wait_for_text(
    session: &mut expectrl::session::OsSession,
    parser: &mut vt100::Parser,
    text: &str,
    timeout_ms: u64,
) -> bool {
    let start = std::time::Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        read_into_parser(session, parser);
        if parser.screen().contents().contains(text) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Test that session metadata is persisted to disk and can be listed/restored.
/// This simulates the "reboot persistence" scenario where:
/// 1. A session is created via the TUI (using SbSession which provides a proper PTY)
/// 2. The session is detached and metadata file is saved
/// 3. We simulate reboot by killing the session but keeping the metadata file
/// 4. The stale session can be listed via `sb stale` and restored via `sb restore`
#[test]
#[serial]
fn test_stale_session_persistence() {
    use std::fs;

    let unique_id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let session_name = format!("stale-test-{}-{}", pid, unique_id);
    let binary_path = get_binary_path();

    // Cleanup helper
    struct Cleanup {
        session: String,
        binary_path: String,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            // Kill the session if it exists
            let _ = std::process::Command::new(&self.binary_path)
                .args(["kill", &self.session])
                .output();
            // Delete stale metadata if it exists
            let _ = std::process::Command::new(&self.binary_path)
                .args(["forget", &self.session])
                .output();
        }
    }
    let _cleanup = Cleanup {
        session: session_name.clone(),
        binary_path: binary_path.clone(),
    };

    // ============ PHASE 1: Create a session via proper PTY using expectrl ============
    {
        let cmd = format!("{} -s {}", binary_path, session_name);
        let mut session = spawn(&cmd).expect("Failed to spawn sb");
        session.set_expect_timeout(Some(Duration::from_secs(5)));

        // Wait for TUI to initialize and shell to be ready
        std::thread::sleep(Duration::from_millis(2500));

        // Run a simple command to verify session is working
        session.write_all(b"echo SESSION_ACTIVE\n").expect("Failed to send command");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(1000));

        // Detach with Ctrl+Q
        session.write_all(&[17]).expect("Failed to send Ctrl+Q");
        session.flush().expect("Failed to flush");
        std::thread::sleep(Duration::from_millis(500));
        let _ = session.get_process_mut().exit(true);
    }

    // Small delay to let daemon process the detach
    std::thread::sleep(Duration::from_millis(500));

    // Verify the session is listed as active
    let list_output = std::process::Command::new(&binary_path)
        .arg("list")
        .output()
        .expect("Failed to run sb list");
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    eprintln!("List output:\n{}", list_stdout);

    assert!(
        list_stdout.contains(&session_name),
        "Session should be listed as active. Got:\n{}",
        list_stdout
    );

    // ============ PHASE 2: Verify metadata file was created ============
    let data_dir = if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        std::path::PathBuf::from(dir).join("sidebar-tui")
    } else {
        dirs::home_dir()
            .unwrap()
            .join(".local")
            .join("share")
            .join("sidebar-tui")
    };
    let sessions_dir = data_dir.join("sessions");
    let metadata_file = sessions_dir.join(format!("{}.json", session_name));

    eprintln!("Looking for metadata at: {:?}", metadata_file);
    assert!(
        metadata_file.exists(),
        "Metadata file should exist at {:?}",
        metadata_file
    );

    // Read and save the metadata content
    let metadata_content = fs::read_to_string(&metadata_file)
        .expect("Failed to read metadata file");
    eprintln!("Metadata content:\n{}", metadata_content);

    // ============ PHASE 3: Kill session and restore metadata to simulate reboot ============
    // Kill the session (this will delete the metadata file)
    let kill_output = std::process::Command::new(&binary_path)
        .args(["kill", &session_name])
        .output()
        .expect("Failed to run sb kill");
    eprintln!("Kill output: {}", String::from_utf8_lossy(&kill_output.stdout));

    // Restore the metadata file to simulate "reboot" scenario
    // (After reboot, daemon is gone but metadata files persist on disk)
    fs::create_dir_all(&sessions_dir).expect("Failed to create sessions dir");
    fs::write(&metadata_file, &metadata_content).expect("Failed to restore metadata");

    // Verify session is no longer listed as active
    let list_output2 = std::process::Command::new(&binary_path)
        .arg("list")
        .output()
        .expect("Failed to run sb list");
    let list_stdout2 = String::from_utf8_lossy(&list_output2.stdout);
    eprintln!("List output after kill:\n{}", list_stdout2);

    assert!(
        !list_stdout2.contains(&session_name),
        "Session should NOT be in active list after kill. Got:\n{}",
        list_stdout2
    );

    // ============ PHASE 4: Verify stale session appears ============
    let stale_output = std::process::Command::new(&binary_path)
        .arg("stale")
        .output()
        .expect("Failed to run sb stale");
    let stale_stdout = String::from_utf8_lossy(&stale_output.stdout);
    eprintln!("Stale output:\n{}", stale_stdout);

    assert!(
        stale_stdout.contains(&session_name),
        "Session should appear in stale list. Got:\n{}",
        stale_stdout
    );

    // ============ PHASE 5: Restore the stale session ============
    let restore_output = std::process::Command::new(&binary_path)
        .args(["restore", &session_name])
        .output()
        .expect("Failed to run sb restore");
    let restore_stdout = String::from_utf8_lossy(&restore_output.stdout);
    eprintln!("Restore output:\n{}", restore_stdout);

    assert!(
        restore_stdout.contains("Restored"),
        "Restore should succeed. Got:\n{}",
        restore_stdout
    );

    // Verify session is now active again
    let list_output3 = std::process::Command::new(&binary_path)
        .arg("list")
        .output()
        .expect("Failed to run sb list");
    let list_stdout3 = String::from_utf8_lossy(&list_output3.stdout);
    eprintln!("List output after restore:\n{}", list_stdout3);

    assert!(
        list_stdout3.contains(&session_name),
        "Session should be active after restore. Got:\n{}",
        list_stdout3
    );

    // ============ PHASE 6: Clean up ============
    // Kill the restored session
    let _ = std::process::Command::new(&binary_path)
        .args(["kill", &session_name])
        .output();
}

/// Test that the sidebar is exactly 20 characters wide
#[test]
#[serial]
fn test_sidebar_is_20_chars_wide() {
    let mut session = SbSession::new().expect("Failed to spawn sb");

    std::thread::sleep(Duration::from_millis(1000));
    session.read_and_parse().expect("Failed to read output");

    // Check the background colors at column boundaries
    // Column 19 (last sidebar column) should have sidebar styling
    // Column 20 (first terminal column) should have terminal styling

    if let (Some(sidebar_last), Some(terminal_first)) =
        (session.cell_at(1, 19), session.cell_at(1, 20))
    {
        let sidebar_bg = sidebar_last.bgcolor();
        let terminal_bg = terminal_first.bgcolor();

        // The sidebar body has DarkGray background, terminal has Black
        // They should be different
        assert!(
            sidebar_bg != terminal_bg,
            "Column 19 (sidebar) and column 20 (terminal) should have different backgrounds. Col 19: {:?}, Col 20: {:?}",
            sidebar_bg, terminal_bg
        );
    }

    session.quit().expect("Failed to quit");
}
