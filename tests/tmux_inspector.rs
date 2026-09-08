//! Real tmux tests: missing tmux is a failure, never a skipped success.
#![cfg(unix)]
use serde_json::Value;
use std::{
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    dir: PathBuf,
    socket: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = PathBuf::from(format!("/tmp/sb-ti-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            dir.join("tmux.conf"),
            "set -g default-shell /bin/sh\nset -g automatic-rename off\nset -g exit-empty off\n",
        )
        .unwrap();
        Self {
            socket: dir.join("socket"),
            dir,
        }
    }
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .env_clear()
            .env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
            .env("HOME", &self.dir)
            .env("SHELL", "/bin/sh")
            .env("LC_ALL", "C")
            .args([
                "-S",
                self.socket.to_str().unwrap(),
                "-f",
                self.dir.join("tmux.conf").to_str().unwrap(),
            ])
            .args(args)
            .output()
            .expect("tmux 3.6a must be installed");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    fn cli(&self) -> Command {
        let binary = std::env::var("SB_TMUX_TEST_BINARY")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_sb").into());
        let mut cmd = Command::new(binary);
        cmd.env_remove("TMUX")
            .env("XDG_DATA_HOME", &self.dir)
            .env("XDG_RUNTIME_DIR", &self.dir);
        cmd.args(["tmux", "--socket-path", self.socket.to_str().unwrap()]);
        cmd
    }
    fn inspect(&self, action: &str) -> Value {
        let out = self.cli().args([action, "--json"]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn inventory(&self) -> String {
        self.tmux(&["list-panes", "-a", "-F", "#{session_id}|#{window_id}|#{pane_id}|#{pane_pid}|#{window_active}|#{pane_active}|#{window_layout}|#{window_name}"])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this fixture's private socket is ever cleaned up, even on assertion failure.
        let _ = Command::new("tmux")
            .args(["-N", "-S", self.socket.to_str().unwrap(), "kill-server"])
            .output();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn error(out: Output, expected: &str) {
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(expected),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn duplicate_names_linked_membership_and_read_only_inventory() {
    let f = Fixture::new();
    let name = "duplicate é🙂\t\n:\\#{pane_id};'\u{1b}";
    // new-session expands -n formats: escape hashes to store the literal name.
    let literal = name.replace('#', "##");
    f.tmux(&[
        "new-session",
        "-d",
        "-s",
        "first",
        "-n",
        &literal,
        "sleep 300",
    ]);
    f.tmux(&[
        "new-session",
        "-d",
        "-s",
        "second",
        "-n",
        &literal,
        "sleep 300",
    ]);
    f.tmux(&["link-window", "-d", "-s", "first:0", "-t", "second:1"]);
    let before = f.inventory();
    let report = f.inspect("doctor");
    let inv = &report["inventory"];
    assert_eq!(inv["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(inv["windows"].as_array().unwrap().len(), 2);
    assert_eq!(inv["memberships"].as_array().unwrap().len(), 3);
    assert_eq!(inv["panes"].as_array().unwrap().len(), 2);
    assert_ne!(inv["windows"][0]["id"], inv["windows"][1]["id"]);
    for w in inv["windows"].as_array().unwrap() {
        assert_eq!(w["name"], name);
    }
    for action in ["list-sessions", "list-windows", "list-panes"] {
        f.inspect(action);
        let out = f.cli().arg(action).output().unwrap();
        assert!(out.status.success());
        assert!(!out.stdout.contains(&0x1b));
    }
    assert_eq!(before, f.inventory());
    // Context routing uses the same server; explicit selectors override malformed context.
    assert!(
        f.cli()
            .env("TMUX", "malformed")
            .arg("doctor")
            .output()
            .unwrap()
            .status
            .success()
    );
    let binary =
        std::env::var("SB_TMUX_TEST_BINARY").unwrap_or_else(|_| env!("CARGO_BIN_EXE_sb").into());
    let out = Command::new(binary)
        .env("TMUX", format!("{},123,0", f.socket.display()))
        .args(["tmux", "list-sessions", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    // External deletion and same-socket restart never retain old inventory/observations.
    f.tmux(&["kill-session", "-t", "first"]);
    assert_eq!(
        f.inspect("list-sessions")["sessions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    f.tmux(&["kill-server"]);
    std::thread::sleep(std::time::Duration::from_millis(100));
    f.tmux(&["new-session", "-d", "-s", "replacement", "sleep 300"]);
    let replacement = f.inspect("doctor");
    assert_ne!(inv["server"], replacement["inventory"]["server"]);
    assert_eq!(
        replacement["inventory"]["sessions"][0]["name"],
        "replacement"
    );
    f.tmux(&["kill-session", "-t", "replacement"]);
    assert!(
        f.inspect("list-sessions")["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn missing_socket_dependency_old_version_and_conflicts() {
    let f = Fixture::new();
    error(
        f.cli().arg("doctor").output().unwrap(),
        "no server was created",
    );
    assert!(!f.socket.exists());
    error(
        f.cli().env("PATH", &f.dir).arg("doctor").output().unwrap(),
        "Install tmux",
    );
    error(
        f.cli()
            .args(["--socket-name", "conflict", "doctor"])
            .output()
            .unwrap(),
        "cannot be used with",
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(f.dir.join("tmux"), "#!/bin/sh\nprintf 'tmux 2.0\\n'\n").unwrap();
    std::fs::set_permissions(f.dir.join("tmux"), std::fs::Permissions::from_mode(0o700)).unwrap();
    error(
        f.cli().env("PATH", &f.dir).arg("doctor").output().unwrap(),
        "Unsupported tmux version",
    );
    // A non-socket path must not be treated as an empty server or overwritten.
    std::fs::write(&f.socket, "not a socket").unwrap();
    error(
        f.cli().arg("doctor").output().unwrap(),
        "no server was created",
    );
    assert_eq!(std::fs::read_to_string(&f.socket).unwrap(), "not a socket");
}
