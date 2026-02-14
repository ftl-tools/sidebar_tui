//! PTY management for terminal emulation.
//!
//! This module handles spawning a shell in a pseudo-terminal and
//! managing communication with it via channels.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use color_eyre::Result;
use color_eyre::eyre::{Context, eyre};
use portable_pty::{CommandBuilder, Child, MasterPty, PtySize, native_pty_system};

/// Extension trait to convert anyhow::Error to eyre::Error.
trait AnyhowToEyre<T> {
    fn to_eyre(self) -> Result<T>;
}

impl<T> AnyhowToEyre<T> for std::result::Result<T, anyhow::Error> {
    fn to_eyre(self) -> Result<T> {
        self.map_err(|e| eyre!("{}", e))
    }
}

/// Messages sent from the PTY reader thread to the main thread.
#[derive(Debug)]
pub enum PtyEvent {
    /// Raw output data from the PTY.
    Output(Vec<u8>),
    /// The PTY process has exited.
    Exited,
}

/// Handle to a running PTY session.
pub struct PtyHandle {
    /// Receiver for PTY output events.
    pub rx: Receiver<PtyEvent>,
    /// Writer to send input to the PTY.
    writer: Box<dyn Write + Send>,
    /// The child process handle.
    child: Box<dyn Child + Send + Sync>,
    /// Handle to the reader thread.
    _reader_thread: JoinHandle<()>,
    /// The master PTY for resize operations.
    master: Box<dyn MasterPty + Send>,
}

impl PtyHandle {
    /// Write raw bytes to the PTY (send input to the shell).
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writer
            .write_all(data)
            .context("failed to write to PTY")?;
        self.writer.flush().context("failed to flush PTY writer")?;
        Ok(())
    }

    /// Resize the PTY to the given dimensions.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .to_eyre()
            .context("failed to resize PTY")?;
        Ok(())
    }

    /// Check if the child process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the child process to exit.
    pub fn wait(&mut self) -> Result<()> {
        self.child.wait().context("failed to wait for PTY child")?;
        Ok(())
    }
}

/// Spawn a new shell in a PTY.
///
/// # Arguments
/// * `rows` - Number of rows for the terminal
/// * `cols` - Number of columns for the terminal
/// * `cwd` - Working directory for the shell (uses current dir if None)
pub fn spawn_shell(rows: u16, cols: u16, cwd: Option<PathBuf>) -> Result<PtyHandle> {
    let pty_system = native_pty_system();

    // Open PTY with the given size
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .to_eyre()
        .context("failed to open PTY")?;

    // Determine the shell to use
    let shell = get_user_shell();
    let mut cmd = CommandBuilder::new(&shell);

    // Set working directory
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    } else {
        let current_dir = std::env::current_dir().context("failed to get current directory")?;
        cmd.cwd(current_dir);
    }

    // Spawn the shell
    let child = pair
        .slave
        .spawn_command(cmd)
        .to_eyre()
        .context("failed to spawn shell")?;

    // Drop the slave - we only communicate through the master
    drop(pair.slave);

    // Get reader and writer from master
    let reader = pair
        .master
        .try_clone_reader()
        .to_eyre()
        .context("failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .to_eyre()
        .context("failed to take PTY writer")?;

    // Create channel for output
    let (tx, rx) = mpsc::channel::<PtyEvent>();

    // Spawn reader thread
    let reader_thread = spawn_reader_thread(reader, tx);

    Ok(PtyHandle {
        rx,
        writer,
        child,
        _reader_thread: reader_thread,
        master: pair.master,
    })
}

/// Spawn a background thread that reads from the PTY and sends events via channel.
fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    tx: Sender<PtyEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    // EOF - PTY closed
                    let _ = tx.send(PtyEvent::Exited);
                    break;
                }
                Ok(n) => {
                    let data = buffer[..n].to_vec();
                    if tx.send(PtyEvent::Output(data)).is_err() {
                        // Receiver dropped, exit
                        break;
                    }
                }
                Err(e) => {
                    // Read error - likely PTY closed
                    eprintln!("PTY read error: {}", e);
                    let _ = tx.send(PtyEvent::Exited);
                    break;
                }
            }
        }
    })
}

/// Get the user's default shell.
fn get_user_shell() -> String {
    // Try SHELL environment variable first
    if let Ok(shell) = std::env::var("SHELL") {
        return shell;
    }

    // Fall back to common shells
    #[cfg(unix)]
    {
        // Check if zsh exists (common on macOS)
        if std::path::Path::new("/bin/zsh").exists() {
            return "/bin/zsh".to_string();
        }
        // Fall back to bash
        if std::path::Path::new("/bin/bash").exists() {
            return "/bin/bash".to_string();
        }
        // Ultimate fallback
        "/bin/sh".to_string()
    }

    #[cfg(windows)]
    {
        // PowerShell on Windows
        "powershell.exe".to_string()
    }

    #[cfg(not(any(unix, windows)))]
    {
        "sh".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_get_user_shell_returns_valid_path() {
        let shell = get_user_shell();
        assert!(!shell.is_empty(), "Shell should not be empty");
        // On Unix systems, shell path should start with /
        #[cfg(unix)]
        assert!(
            shell.starts_with('/'),
            "Unix shell should be absolute path: {}",
            shell
        );
    }

    #[test]
    fn test_spawn_shell_creates_pty() {
        let handle = spawn_shell(24, 80, None);
        assert!(handle.is_ok(), "Failed to spawn shell: {:?}", handle.err());
    }

    #[test]
    fn test_pty_can_write_and_receive_output() {
        let mut handle = spawn_shell(24, 80, None).expect("Failed to spawn shell");

        // Send a simple echo command
        handle
            .write(b"echo TESTOUTPUT123\r")
            .expect("Failed to write to PTY");

        // Wait for output with timeout
        let mut received_output = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            match handle.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(PtyEvent::Output(data)) => {
                    let output = String::from_utf8_lossy(&data);
                    if output.contains("TESTOUTPUT123") {
                        received_output = true;
                        break;
                    }
                }
                Ok(PtyEvent::Exited) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }

        assert!(received_output, "Should receive echo output from PTY");
    }

    #[test]
    fn test_pty_resize() {
        let handle = spawn_shell(24, 80, None).expect("Failed to spawn shell");

        // Resize should succeed
        let result = handle.resize(40, 120);
        assert!(result.is_ok(), "Resize failed: {:?}", result.err());
    }

    #[test]
    fn test_pty_is_running() {
        let mut handle = spawn_shell(24, 80, None).expect("Failed to spawn shell");

        // Should be running initially
        assert!(handle.is_running(), "PTY should be running after spawn");
    }

    #[test]
    fn test_pty_with_custom_cwd() {
        let temp_dir = std::env::temp_dir();
        let handle = spawn_shell(24, 80, Some(temp_dir.clone()));
        assert!(
            handle.is_ok(),
            "Failed to spawn shell with custom cwd: {:?}",
            handle.err()
        );
    }
}
