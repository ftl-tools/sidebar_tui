#![cfg(unix)]
use expectrl::{Expect, session::OsSession};
use std::{
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, Instant},
};

struct Fixture {
    dir: PathBuf,
    socket: PathBuf,
    binary: String,
}
impl Fixture {
    fn new() -> Self {
        let dir = PathBuf::from(format!("/tmp/sb-ts-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            socket: dir.join("socket"),
            dir,
            binary: std::env::var("SB_TMUX_TEST_BINARY")
                .unwrap_or_else(|_| env!("CARGO_BIN_EXE_sb").into()),
        }
    }
    fn env(&self, c: &mut Command) {
        c.env_clear()
            .env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
            .env("HOME", &self.dir)
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .env("LC_ALL", "en_US.UTF-8")
            .env("XDG_DATA_HOME", &self.dir)
            .env("XDG_RUNTIME_DIR", &self.dir);
    }
    fn cli(&self) -> Command {
        let mut c = Command::new(&self.binary);
        self.env(&mut c);
        c.args(["tmux", "--socket-path", self.socket.to_str().unwrap()]);
        c
    }
    fn command(&self) -> Command {
        let mut c = Command::new("tmux");
        self.env(&mut c);
        c.args(["-u", "-S", self.socket.to_str().unwrap(), "-f", "/dev/null"]);
        c
    }
    fn tmux(&self, args: &[&str]) -> String {
        success(
            self.command()
                .args(args)
                .output()
                .expect("tmux 3.6a required"),
        )
    }
    fn launch(&self) {
        success(
            self.cli()
                .args([
                    "sidebar-demo",
                    "--cwd",
                    self.dir.to_str().unwrap(),
                    "--enable-binding",
                ])
                .output()
                .unwrap(),
        );
    }
    fn panels(&self) -> Vec<(String, String, String)> {
        self.tmux(&[
            "list-panes",
            "-a",
            "-F",
            "#{window_id} #{pane_id} #{pane_pid} #{@sb_demo_panel}",
        ])
        .lines()
        .filter(|l| l.ends_with("sidebar-step3-v1"))
        .map(|l| {
            let f: Vec<_> = l.split_whitespace().collect();
            (f[0].into(), f[1].into(), f[2].into())
        })
        .collect()
    }
    fn workers(&self) -> String {
        self.tmux(&[
            "list-panes",
            "-a",
            "-F",
            "#{window_id} #{pane_id} #{pane_pid} #{@sb_demo_panel}",
        ])
        .lines()
        .filter(|l| !l.ends_with("sidebar-step3-v1"))
        .collect::<Vec<_>>()
        .join("\n")
    }
    fn attach(&self) -> OsSession {
        let mut c = self.command();
        c.args(["attach-session", "-t", "sidebar-demo"]);
        let mut s = expectrl::Session::spawn(c).unwrap();
        s.set_expect_timeout(Some(Duration::from_secs(15)));
        s
    }
    fn active(&self) -> String {
        self.tmux(&[
            "display-message",
            "-p",
            "-t",
            "sidebar-demo",
            "#{window_id} #{pane_id} #{window_zoomed_flag}",
        ])
        .trim()
        .into()
    }
    fn wait_active(&self, prefix: &str) {
        let start = Instant::now();
        loop {
            if self.active().starts_with(prefix) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "expected {prefix}, got {}",
                self.active()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    fn wait_panels(&self, n: usize) {
        let start = Instant::now();
        loop {
            if self.panels().len() == n {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "panel count mismatch"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command().args(["-N", "kill-server"]).output();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn success(o: Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
fn wait(s: &mut OsSession, parser: &mut vt100::Parser, text: &str) {
    let start = Instant::now();
    loop {
        let mut b = [0; 16384];
        while let Ok(n) = s.try_read(&mut b) {
            if n == 0 {
                break;
            }
            parser.process(&b[..n]);
        }
        if parser.screen().contents().contains(text) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "Missing {text}: {}",
            parser.screen().contents()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}
#[test]
fn concurrent_enrollment_close_crash_reopen_preserves_workloads() {
    let f = Fixture::new();
    let mut launches: Vec<_> = (0..3)
        .map(|_| {
            f.cli()
                .args([
                    "sidebar-demo",
                    "--cwd",
                    f.dir.to_str().unwrap(),
                    "--enable-binding",
                ])
                .spawn()
                .unwrap()
        })
        .collect();
    for child in &mut launches {
        assert!(child.wait().unwrap().success());
    }
    f.wait_panels(2);
    let workers = f.workers();
    let mut jobs: Vec<_> = (0..4)
        .map(|_| f.cli().arg("sidebar-show").spawn().unwrap())
        .collect();
    for child in &mut jobs {
        assert!(child.wait().unwrap().success());
    }
    assert_eq!(f.panels().len(), 2);
    assert_eq!(workers, f.workers());
    let old = f.panels()[0].clone();
    // Kill only the fixture-owned Sidebar display process, never its working sibling.
    assert_eq!(
        unsafe { libc::kill(old.2.parse().unwrap(), libc::SIGKILL) },
        0
    );
    f.wait_panels(1);
    success(f.cli().arg("sidebar-show").output().unwrap());
    f.wait_panels(2);
    assert!(!f.panels().iter().any(|p| p.1 == old.1));
    assert_eq!(workers, f.workers());
    success(
        f.cli()
            .args(["sidebar-close", "--disable-binding"])
            .output()
            .unwrap(),
    );
    f.wait_panels(0);
    assert_eq!(workers, f.workers());
    assert!(
        !f.command()
            .args(["list-keys", "-T", "root", "F12"])
            .output()
            .unwrap()
            .status
            .success()
    );
    success(f.cli().arg("sidebar-show").output().unwrap());
    f.wait_panels(2);
    assert_eq!(workers, f.workers());
}
#[test]
fn native_focus_no_leak_zoom_resize_and_two_clients() {
    let f = Fixture::new();
    f.launch();
    let workers = f.workers();
    for (pane, name) in [("%0", "editor_input"), ("%1", "logs_input")] {
        let cmd = format!("stty raw -echo; cat > {}", f.dir.join(name).display());
        f.tmux(&["send-keys", "-t", pane, &cmd, "Enter"]);
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut first = f.attach();
    let mut parser = vt100::Parser::new(24, 80, 0);
    wait(&mut first, &mut parser, "Return to work");
    let mut second = f.attach();
    std::thread::sleep(Duration::from_millis(200));
    let panels = f.panels();
    let editor = panels.iter().find(|p| p.0 == "@0").unwrap();
    let logs = panels.iter().find(|p| p.0 == "@1").unwrap();
    first.send("\t").unwrap();
    f.wait_active("@0 %0");
    // F12 must focus synchronously: immediately following navigation must not reach cat.
    first.send("\x1b[24~j\r").unwrap();
    f.wait_active(&format!("@1 {}", logs.1));
    for _ in 0..3 {
        first.send("k\r").unwrap();
        f.wait_active(&format!("@0 {}", editor.1));
        first.send("j\r").unwrap();
        f.wait_active(&format!("@1 {}", logs.1));
    }
    first.send("UNSUPPORTED").unwrap();
    assert_eq!(
        f.tmux(&["list-clients", "-F", "#{window_id} #{pane_id}"])
            .lines()
            .collect::<Vec<_>>(),
        vec![format!("@1 {}", logs.1); 2]
    );
    first.send("\t").unwrap();
    f.wait_active("@1 %1");
    f.tmux(&["resize-pane", "-Z", "-t", "%1"]);
    assert!(f.active().ends_with('1'));
    first.send("\x1b[24~").unwrap();
    f.wait_active(&format!("@1 {} 0", logs.1));
    f.tmux(&["resize-window", "-t", "@1", "-x", "100", "-y", "32"]);
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        f.tmux(&["capture-pane", "-p", "-t", &logs.1])
            .contains("Return to work")
    );
    // Structural copy-mode smoke is not a substitute for the required manual mouse/copy evidence.
    f.tmux(&["copy-mode", "-t", "%1"]);
    assert_eq!(
        f.tmux(&["display-message", "-p", "-t", "%1", "#{pane_in_mode}"])
            .trim(),
        "1"
    );
    f.tmux(&["send-keys", "-t", "%1", "-X", "cancel"]);
    assert_eq!(
        std::fs::metadata(f.dir.join("editor_input")).unwrap().len(),
        0
    );
    assert_eq!(
        std::fs::metadata(f.dir.join("logs_input")).unwrap().len(),
        0
    );
    first.send("q").unwrap();
    f.wait_panels(1);
    assert_eq!(workers, f.workers());
    success(
        f.cli()
            .args(["sidebar-show", "--window", "@1"])
            .output()
            .unwrap(),
    );
    f.wait_panels(2);
    assert_eq!(workers, f.workers());
    first.send("\x02d").unwrap();
    first.expect(expectrl::Eof).unwrap();
    second.send("\x02d").unwrap();
    second.expect(expectrl::Eof).unwrap();
}
#[test]
fn stale_role_marker_cannot_authorize_killing_replacement_work() {
    let f = Fixture::new();
    f.launch();
    let panel = f.panels()[0].clone();
    f.tmux(&["respawn-pane", "-k", "-t", &panel.1, "/bin/sh"]);
    let pid = f.tmux(&["display-message", "-p", "-t", &panel.1, "#{pane_pid}"]);
    assert_ne!(pid.trim(), panel.2);
    let out = f.cli().arg("sidebar-close").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("replaced externally"));
    assert_eq!(
        pid,
        f.tmux(&["display-message", "-p", "-t", &panel.1, "#{pane_pid}"])
    );
}
#[test]
fn opt_in_rejects_repeat_binding_collision_and_implicit_socket() {
    let f = Fixture::new();
    success(
        f.cli()
            .args(["sidebar-demo", "--cwd", f.dir.to_str().unwrap()])
            .output()
            .unwrap(),
    );
    f.tmux(&[
        "bind-key",
        "-r",
        "-T",
        "root",
        "F12",
        "display-message",
        "user repeat binding",
    ]);
    let before = f.tmux(&["list-keys", "-T", "root", "F12"]);
    assert!(
        !f.cli()
            .args([
                "sidebar-demo",
                "--cwd",
                f.dir.to_str().unwrap(),
                "--enable-binding"
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(before, f.tmux(&["list-keys", "-T", "root", "F12"]));
    let mut c = Command::new(&f.binary);
    f.env(&mut c);
    c.env("TMUX", format!("{},123,0", f.socket.display()))
        .args(["tmux", "sidebar-show"]);
    let out = c.output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("explicit --socket-path"));
}
#[test]
fn refuses_unowned_servers_and_preserves_later_binding_edits() {
    let f = Fixture::new();
    f.tmux(&["new-session", "-d", "-s", "user-work"]);
    let before = f.workers();
    assert!(
        !f.cli()
            .args(["sidebar-demo", "--cwd", f.dir.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(before, f.workers());
    // Only fixture server is retired between independent scenarios.
    f.tmux(&["kill-server"]);
    std::thread::sleep(Duration::from_millis(150));
    f.launch();
    let workers = f.workers();
    f.tmux(&[
        "bind-key",
        "-T",
        "root",
        "F12",
        "display-message",
        "user replacement",
    ]);
    let binding = f.tmux(&["list-keys", "-T", "root", "F12"]);
    assert!(
        !f.cli()
            .args(["sidebar-close", "--disable-binding"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(binding, f.tmux(&["list-keys", "-T", "root", "F12"]));
    assert_eq!(workers, f.workers());
}
