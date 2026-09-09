use super::*;

fn wait_for(client: &mut SbClient, text: &str) {
    // Iteration counts multiplied nested waits into a ~22-second silent stall.
    // The shared readiness check uses a single three-second deadline instead.
    client.wait_for_screen(text);
}

#[test]
fn test_tmux_terminology_workflow() {
    let env = TestEnv::setup();
    let mut client = SbClient::new(&env).unwrap();
    wait_for(&mut client, "Switch session");
    client.send_ctrl_b().unwrap();
    wait_for(&mut client, "Kill window");
    assert!(!client.screen_contents().contains("Workspace"));

    // The old E2E flows send retired Ctrl+W/n bindings. Exercise current s/C bindings.
    client.send("s").unwrap();
    wait_for(&mut client, "Kill session");
    client.send("C").unwrap();
    client.send("Project").unwrap();
    client.send_enter().unwrap();
    // "Project" also appears in the draft. Waiting only for that text raced the
    // create request once blanket sleeps were removed; wait for committed IPC state.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        client.read_and_parse().unwrap();
        let sessions = env
            .iso_command()
            .args(["session", "list"])
            .output()
            .unwrap();
        assert!(sessions.status.success());
        if String::from_utf8_lossy(&sessions.stdout).contains("Project") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Session was not committed: {}",
            client.screen_contents()
        );
    }
    wait_for(&mut client, "Kill session");

    client.send_down_arrow().unwrap();
    client.send_enter().unwrap();
    wait_for(&mut client, "┌ Project");
    // Session selection returns to sidebar command mode.
    client.send("c").unwrap();
    client.send("api-server").unwrap();
    client.send_enter().unwrap();
    wait_for(&mut client, "Switch session");
    wait_for(&mut client, "api-server");

    client.send_ctrl_b().unwrap();
    wait_for(&mut client, "Kill window");
    client.send("&").unwrap();
    wait_for(&mut client, "Kill this window");
    client.send("n").unwrap();
    wait_for(&mut client, "Kill window");
    client.send("d").unwrap();
    wait_for(&mut client, "Detach Sidebar TUI?");
    client.send("y").unwrap();
    // Exit is observable; do not guess when the detach request has completed.
    use expectrl::Expect;
    client.window.expect(expectrl::Eof).unwrap();

    let windows = env.iso_command().args(["list-windows"]).output().unwrap();
    assert!(windows.status.success());
    assert!(
        String::from_utf8_lossy(&windows.stdout).contains("api-server"),
        "Detach must leave windows running"
    );
    let killed = env
        .iso_command()
        .args(["session", "kill", "Project"])
        .output()
        .unwrap();
    assert!(killed.status.success());
    let windows = env.iso_command().args(["list-windows"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&windows.stdout).contains("api-server"));
}
