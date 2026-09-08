//! Installed-binary-capable PTY acceptance: missing tmux does not prevent legacy launch.
#![cfg(unix)]
use expectrl::Expect;
use std::{path::PathBuf, process::Command, time::Duration};
struct Legacy {
    dir: PathBuf,
    binary: String,
}
impl Legacy {
    fn command(&self) -> Command {
        let mut c = Command::new(&self.binary);
        c.env("XDG_DATA_HOME", &self.dir)
            .env("XDG_RUNTIME_DIR", &self.dir)
            .env("PATH", &self.dir)
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .env_remove("TMUX");
        c
    }
}
impl Drop for Legacy {
    fn drop(&mut self) {
        // This legacy server was explicitly started in this fixture's private runtime only.
        let _ = self.command().arg("shutdown").output();
        std::thread::sleep(Duration::from_millis(200));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
#[test]
fn missing_tmux_still_launches_legacy_in_pty() {
    let f = Legacy {
        dir: PathBuf::from(format!("/tmp/sb-legacy-{:016x}", rand::random::<u64>())),
        binary: std::env::var("SB_TMUX_TEST_BINARY")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_sb").into()),
    };
    std::fs::create_dir(&f.dir).unwrap();
    let missing = f.command().args(["tmux", "doctor"]).output().unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Install tmux"));
    let mut launch = f.command();
    launch.args(["--window", "legacy-without-tmux"]);
    let mut client = expectrl::Session::spawn(launch).unwrap();
    client.set_expect_timeout(Some(Duration::from_secs(15)));
    let mut parser = vt100::Parser::new(24, 80, 0);
    let wait = |client: &mut expectrl::session::OsSession, parser: &mut vt100::Parser, text: &str| {
        for _ in 0..150 {
            let mut buf = [0; 8192];
            while let Ok(n) = client.try_read(&mut buf) {
                if n == 0 { break; }
                parser.process(&buf[..n]);
            }
            if parser.screen().contents().contains(text) { return; }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("Missing {text:?}: {}", parser.screen().contents());
    };
    wait(&mut client, &mut parser, "Switch session");
    client.send("\x02").unwrap();
    wait(&mut client, &mut parser, "Kill window");
    client.send("d").unwrap();
    wait(&mut client, &mut parser, "Detach Sidebar TUI?");
    client.send("y").unwrap();
    client.expect(expectrl::Eof).unwrap();
}
