#![cfg(unix)]
#[path = "support/test_paths.rs"]
mod test_paths;
use expectrl::{Expect, session::OsSession};
use sidebar_tui::tmux::{Adapter, PaneTarget, Socket};
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

struct Fixture {
    dir: PathBuf,
    socket: PathBuf,
    binary: String,
}
impl Fixture {
    fn new() -> Self {
        // Keep resources under the watchdog's private root for timeout cleanup.
        let dir = test_paths::private_dir("sb-tc");
        std::fs::create_dir(&dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        // A private HOME also protects explicit server creation from the user's config.
        std::fs::write(dir.join(".tmux.conf"), "set -g default-shell /bin/sh\nset -g automatic-rename off\nset -g exit-empty off\nset -g status off\n").unwrap();
        Self {
            socket: dir.join("socket"),
            dir,
            binary: std::env::var("SB_TMUX_TEST_BINARY")
                .unwrap_or_else(|_| env!("CARGO_BIN_EXE_sb").into()),
        }
    }
    fn env(&self, c: &mut Command) {
        c.env_clear()
            .env(
                "PATH",
                format!(
                    "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
                    self.dir.display()
                ),
            )
            .env("HOME", &self.dir)
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .env("LC_ALL", "en_US.UTF-8")
            .env("XDG_DATA_HOME", &self.dir)
            .env("XDG_RUNTIME_DIR", &self.dir);
    }
    fn tmux_command(&self) -> Command {
        let mut c = Command::new("tmux");
        self.env(&mut c);
        c.args([
            "-u",
            "-S",
            self.socket.to_str().unwrap(),
            "-f",
            self.dir.join(".tmux.conf").to_str().unwrap(),
        ]);
        c
    }
    fn tmux(&self, args: &[&str]) -> String {
        let o = self
            .tmux_command()
            .args(args)
            .output()
            .expect("tmux 3.6a required");
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    }
    fn seed(&self) {
        self.tmux(&["new-session", "-d", "-s", "one", "-n", "duplicate"]);
        self.tmux(&["new-session", "-d", "-s", "two", "-n", "duplicate"]);
    }
    fn adapter(&self) -> Adapter {
        Adapter::new(Socket::Path(self.socket.to_str().unwrap().into()))
    }
    fn inventory(&self) -> String {
        self.tmux(&["list-panes", "-a", "-F", "#{session_id}:#{window_id}:#{pane_id}:#{pane_pid}:#{pane_active}:#{window_active}:#{window_layout}"])
    }
    fn chooser(&self, extra: &[&str]) -> OsSession {
        let mut c = Command::new("/bin/sh");
        self.env(&mut c);
        // Positional argv avoids shell interpolation even for adversarial paths.
        c.args(["-c", "before=$(/bin/stty -g); \"$@\"; after=$(/bin/stty -g); if [ \"$before\" = \"$after\" ]; then echo TERMINAL_RESTORED; else echo TERMINAL_BROKEN; fi", "fixture", &self.binary, "tmux", "--socket-path", self.socket.to_str().unwrap()]).args(extra);
        let mut s = expectrl::Session::spawn(c).unwrap();
        s.set_expect_timeout(Some(Duration::from_secs(15)));
        s
    }
    fn attach(&self) -> OsSession {
        let mut c = self.tmux_command();
        c.args(["attach-session", "-t", "one"]);
        let mut s = expectrl::Session::spawn(c).unwrap();
        s.set_expect_timeout(Some(Duration::from_secs(15)));
        s
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.tmux_command().args(["-N", "kill-server"]).output();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn wait(s: &mut OsSession, parser: &mut vt100::Parser, text: &str) {
    let start = Instant::now();
    loop {
        let mut buf = [0; 16384];
        while let Ok(n) = s.try_read(&mut buf) {
            if n == 0 {
                break;
            }
            parser.process(&buf[..n]);
        }
        if parser.screen().contents().contains(text) {
            if text == "TERMINAL_RESTORED" {
                assert!(
                    !parser.screen().alternate_screen(),
                    "alternate screen was not restored"
                );
            }
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "Missing {text:?}: {}",
            parser.screen().contents()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn parser() -> vt100::Parser {
    vt100::Parser::new(24, 80, 0)
}
fn choose(s: &mut OsSession, p: &mut vt100::Parser, pane: &str) {
    s.send(format!("/{pane}")).unwrap();
    wait(s, p, &format!("Search (typing): {pane}"));
    wait(s, p, "Finish search (no attach)");
    s.send("\r").unwrap();
    wait(s, p, &format!("Search: {pane}"));
    s.send("\r").unwrap();
}
#[test]
fn browse_refresh_delete_restart_cancel_restore() {
    let f = Fixture::new();
    f.seed();
    let before = f.inventory();
    let mut s = f.chooser(&[]);
    let mut p = parser();
    wait(&mut s, &mut p, "duplicate");
    wait(&mut s, &mut p, "W* P*");
    s.send("j").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(before, f.inventory());
    f.tmux(&["rename-window", "-t", "two:0", "externally-renamed"]);
    wait(&mut s, &mut p, "externally-renamed");
    f.tmux(&["kill-session", "-t", "two"]);
    wait(&mut s, &mut p, "Selected target disappeared");
    s.send("\r").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(f.tmux(&["list-clients"]).is_empty());
    f.tmux(&["kill-server"]);
    std::thread::sleep(Duration::from_millis(150));
    f.tmux(&["new-session", "-d", "-s", "replacement"]);
    wait(&mut s, &mut p, "replacement");
    s.send("q").unwrap();
    wait(&mut s, &mut p, "TERMINAL_RESTORED");
    s.expect(expectrl::Eof).unwrap();
}
#[test]
fn outside_attach_real_shell_native_detach_and_reopen() {
    let f = Fixture::new();
    f.seed();
    f.tmux(&["split-window", "-d", "-t", "two:0"]);
    let mut s = f.chooser(&[]);
    let mut p = parser();
    wait(&mut s, &mut p, "duplicate");
    choose(&mut s, &mut p, "%2");
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        f.tmux(&["display-message", "-p", "-t", "two:0", "#{pane_id}"])
            .trim(),
        "%2"
    );
    s.send("printf 'NATIVE_%s\\n' SHELL\r").unwrap();
    wait(&mut s, &mut p, "NATIVE_SHELL");
    assert_eq!(
        f.tmux(&["list-clients", "-F", "#{session_name}"]).trim(),
        "two"
    );
    s.send("\x02d").unwrap();
    wait(&mut s, &mut p, "TERMINAL_RESTORED");
    s.expect(expectrl::Eof).unwrap();
    let mut s = f.chooser(&[]);
    let mut p = parser();
    wait(&mut s, &mut p, "duplicate");
    s.send("\x03").unwrap();
    wait(&mut s, &mut p, "TERMINAL_RESTORED");
}
#[test]
fn explicit_empty_creation_cancel_and_cwd() {
    let f = Fixture::new();
    let cwd = f.dir.join("literal #{session_id} ' space");
    std::fs::create_dir(&cwd).unwrap();
    let mut s = f.chooser(&["--cwd", cwd.to_str().unwrap()]);
    let mut p = parser();
    wait(&mut s, &mut p, "No sessions");
    assert!(!f.socket.exists());
    s.send("c").unwrap();
    wait(&mut s, &mut p, "Create a native session");
    s.send("n").unwrap();
    wait(&mut s, &mut p, "No sessions");
    assert!(!f.socket.exists());
    s.send("c").unwrap();
    wait(&mut s, &mut p, "Create a native session");
    s.send("y").unwrap();
    wait(&mut s, &mut p, "Created. Navigate");
    assert_eq!(
        std::fs::canonicalize(
            f.tmux(&["display-message", "-p", "#{pane_current_path}"])
                .trim()
        )
        .unwrap(),
        std::fs::canonicalize(&cwd).unwrap()
    );
    assert!(f.adapter().create_empty(&f.dir).is_err());
    s.send("q").unwrap();
    wait(&mut s, &mut p, "TERMINAL_RESTORED");
}
#[test]
fn inside_switches_intended_client_without_nesting_and_rejects_ambiguity() {
    let f = Fixture::new();
    f.seed();
    let mut first = f.attach();
    let mut p = parser();
    std::thread::sleep(Duration::from_millis(300));
    // Safe fixed fixture paths; shell receives argv through tmux send-keys only in this owned pane.
    first.send(format!("{} tmux\r", f.binary)).unwrap();
    wait(&mut first, &mut p, "duplicate");
    let mut second = f.attach();
    std::thread::sleep(Duration::from_millis(300));
    choose(&mut first, &mut p, "%1");
    wait(&mut first, &mut p, "Ambiguous or missing tmux client");
    let clients = f.tmux(&["list-clients", "-F", "#{client_name} #{session_name}"]);
    assert_eq!(clients.lines().count(), 2);
    assert!(clients.lines().all(|l| l.ends_with("one")));
    let first_name = clients
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let a = f.adapter();
    let fresh = a.snapshot().unwrap();
    assert_eq!(
        a.choose_client(&fresh, "%0", Some(first_name)).unwrap(),
        first_name
    );
    assert!(
        a.choose_client(&fresh, "%0", Some("/dev/not-a-client"))
            .is_err()
    );
    second.send("\x02d").unwrap();
    second.expect(expectrl::Eof).unwrap();
    first.send("\r").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    first.send("printf 'SWITCHED_%s\\n' NATIVE\r").unwrap();
    wait(&mut first, &mut p, "SWITCHED_NATIVE");
    assert_eq!(
        f.tmux(&["list-clients", "-F", "#{session_name}"]).trim(),
        "two"
    );
    first.send("\x02d").unwrap();
    first.expect(expectrl::Eof).unwrap();
}
#[test]
fn failed_native_attach_restores_terminal() {
    let f = Fixture::new();
    f.seed();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("tmux"))
        .find(|p| p.is_file())
        .expect("tmux required");
    // Fault injection after the real selection request; all discovery still uses real tmux.
    let shim = format!(
        "#!/bin/sh\ncase \"$*\" in *attach-session*) echo injected-attach-failure >&2; exit 1;; esac\nexec '{}' \"$@\"\n",
        real.display()
    );
    std::fs::write(f.dir.join("tmux"), shim).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(f.dir.join("tmux"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut s = f.chooser(&[]);
    let mut p = parser();
    wait(&mut s, &mut p, "duplicate");
    choose(&mut s, &mut p, "%1");
    wait(&mut s, &mut p, "TERMINAL_RESTORED");
    s.expect(expectrl::Eof).unwrap();
    assert_eq!(f.adapter().snapshot().unwrap().panes.len(), 2);
}
#[test]
fn fresh_resolution_rejects_deleted_membership_and_reused_ids() {
    let f = Fixture::new();
    f.seed();
    let a = f.adapter();
    let snap = a.snapshot().unwrap();
    let m = &snap.memberships[0];
    let pane = snap.panes.iter().find(|p| p.window == m.window).unwrap();
    let t = PaneTarget {
        server: snap.server.clone(),
        session: m.session.clone(),
        window: m.window.clone(),
        pane: pane.id.clone(),
    };
    f.tmux(&["kill-session", "-t", "one"]);
    assert!(a.resolve_target(&t).is_err());
    f.tmux(&["kill-server"]);
    std::thread::sleep(Duration::from_millis(150));
    f.seed();
    assert!(
        a.resolve_target(&t)
            .unwrap_err()
            .to_string()
            .contains("restarted")
    );
}
