//! Optional real-Terminal.app observation of the isolated native workload test.
//! Enable with SB_TMUX_DESKTOP_ACCEPTANCE=1; never targets an existing user tab.
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub struct Desktop {
    window: u64,
    tty: String,
    socket: PathBuf,
    evidence: PathBuf,
}

impl Desktop {
    pub fn open(socket: &Path) -> Option<Self> {
        if std::env::var("SB_TMUX_DESKTOP_ACCEPTANCE").as_deref() != Ok("1") {
            return None;
        }
        assert!(
            cfg!(target_os = "macos"),
            "Desktop acceptance requires macOS Terminal.app"
        );
        let evidence =
            std::env::temp_dir().join(format!("sb-step3-evidence-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&evidence).unwrap();
        let tmux = String::from_utf8(
            Command::new("/usr/bin/which")
                .arg("tmux")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let out = Command::new("osascript").args(["-e", r#"
            on run argv
                tell application "Terminal"
                    set priorWindows to id of every window
                    set demoTab to do script (quoted form of item 1 of argv) & " -S " & (quoted form of item 2 of argv) & " attach-session -t sidebar-demo"
                    set custom title of demoTab to "Sidebar Step 3 isolated acceptance"
                    set demoTTY to tty of demoTab
                    repeat 20 times
                        repeat with w in windows
                            if (id of w) is not in priorWindows then
                                if (count of tabs of w) is 1 and (tty of selected tab of w) is demoTTY then
                                    return (id of w as text) & " " & demoTTY
                                end if
                            end if
                        end repeat
                        delay 0.1
                    end repeat
                    error "Cannot identify a newly owned Terminal window; refusing to use front window"
                end tell
            end run
        "#, "--", tmux.trim(), socket.to_str().unwrap()]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let result = String::from_utf8(out.stdout).unwrap();
        let (window, tty) = result
            .trim()
            .split_once(' ')
            .expect("Terminal window and tty");
        let desktop = Self {
            window: window.parse().unwrap(),
            tty: tty.into(),
            socket: socket.into(),
            evidence,
        };
        eprintln!(
            "Desktop evidence: {} (owned Terminal window {}, {})",
            desktop.evidence.display(),
            desktop.window,
            desktop.tty
        );
        // Shell startup can lag behind do-script. Do not capture a shell prompt and
        // mistake it for an attached native client; wait for this exact TTY in tmux.
        let start = std::time::Instant::now();
        loop {
            let out = Command::new("tmux")
                .arg("-N")
                .arg("-S")
                .arg(socket)
                .args(["list-clients", "-F", "#{client_tty}"])
                .output()
                .unwrap();
            if out.status.success()
                && String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .any(|tty| tty == desktop.tty)
            {
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(20),
                "Desktop client did not attach"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Some(desktop)
    }

    pub fn resize(&self, columns: u16, rows: u16) {
        let out = Command::new("osascript").args(["-e", &format!(r#"
            tell application "Terminal"
                if (count of tabs of window id {0}) is not 1 or (tty of selected tab of window id {0}) is not "{1}" then error "Owned tab changed"
                set number of columns of selected tab of window id {0} to {2}
                set number of rows of selected tab of window id {0} to {3}
            end tell
        "#, self.window, self.tty, columns, rows)]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn capture(&self, label: &str) {
        // Capture only our new window: a full desktop screenshot could expose unrelated work.
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Terminal's front window may still be an older window when it is inactive.
        // Revalidate the exact new tab before capturing, never assume front means ours.
        let owned = Command::new("osascript").args(["-e", &format!(
            "tell application \"Terminal\" to return ((count of tabs of window id {0}) is 1 and (tty of selected tab of window id {0}) is \"{1}\")", self.window, self.tty
        )]).output().unwrap();
        assert!(
            owned.status.success() && String::from_utf8_lossy(&owned.stdout).trim() == "true",
            "Owned Terminal tab changed; refusing capture"
        );
        let image = self.evidence.join(format!("{label}.png"));
        let out = Command::new("screencapture")
            .args(["-x", "-l", &self.window.to_string()])
            .arg(image)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "tell application \"Terminal\" to get contents of selected tab of window id {}",
                    self.window
                ),
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        std::fs::write(self.evidence.join(format!("{label}.txt")), out.stdout).unwrap();
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        // Detach only the client from the tab we created, then exit its disposable shell.
        let out = Command::new("tmux")
            .arg("-N")
            .arg("-S")
            .arg(&self.socket)
            .args(["detach-client", "-t", &self.tty])
            .output();
        if out.is_ok_and(|out| out.status.success()) {
            let _ = Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        r#"
                tell application "Terminal"
                    if exists window id {0} then
                        if (count of tabs of window id {0}) is 1 and (tty of selected tab of window id {0}) is "{1}" then
                            do script "exit" in selected tab of window id {0}
                            delay 0.2
                            if exists window id {0} then
                                if (count of tabs of window id {0}) is 1 and (tty of selected tab of window id {0}) is "{1}" then close window id {0} saving no
                            end if
                        end if
                    end if
                end tell
            "#,
                        self.window, self.tty
                    ),
                ])
                .output();
        }
    }
}
