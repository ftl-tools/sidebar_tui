//! Tests of the test harness itself: idle and continuous output must both be bounded.
use super::*;

fn raw_client(command: &str) -> SbClient {
    let iso = TestIsolation::new();
    let mut cmd = std::process::Command::new("/bin/sh");
    iso.apply(&mut cmd);
    cmd.args(["-c", command]);
    SbClient {
        window: Session::spawn(cmd).unwrap(),
        parser: vt100::Parser::new(24, 80, 0),
        window_name: "harness-only".into(),
        iso,
    }
}

#[test]
fn idle_read_does_not_wait_eight_hundred_ms() {
    let mut client = raw_client("sleep 5");
    let start = Instant::now();
    client.read_and_parse().unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(300),
        "idle read took {:?}",
        start.elapsed()
    );
    client.iso.cleanup();
}

#[test]
fn continuous_output_cannot_bypass_read_deadline() {
    let mut client = raw_client("while :; do printf 'HARNESS_BUSY\\n'; done");
    client.wait_for_screen("HARNESS_BUSY");
    let start = Instant::now();
    client.read_and_parse().unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "busy read took {:?}",
        start.elapsed()
    );
    client.iso.cleanup();
}

#[test]
fn missing_text_respects_callers_short_deadline() {
    let mut client = raw_client("sleep 5");
    let start = Instant::now();
    assert!(!wait_for_text(
        &mut client.window,
        &mut client.parser,
        "NOT_PRESENT",
        40
    ));
    assert!(
        start.elapsed() < Duration::from_millis(150),
        "nested drain exceeded the caller's budget"
    );
    client.iso.cleanup();
}

#[test]
fn ready_screen_returns_without_blind_wait() {
    let mut client = raw_client("printf HARNESS_READY; sleep 5");
    client.wait_for_screen("HARNESS_READY");
    let start = Instant::now();
    client.wait_for_screen("HARNESS_READY");
    assert!(start.elapsed() < Duration::from_millis(300));
    client.iso.cleanup();
}
