//! Migration coverage: old persisted/wire names must survive the public API rename.
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sidebar_tui::server::{ClientMessage, ServerResponse, SessionMetadata, WindowMetadata};
use std::process::Command;

fn round_trip<T: DeserializeOwned + Serialize>(fixture: Value) {
    let decoded: T = serde_json::from_value(fixture.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), fixture);
}

#[test]
fn legacy_client_protocol_remains_compatible() {
    for fixture in [
        json!({"Attach":{"session_name":"shell","rows":24,"cols":80,"cwd":null}}),
        json!({"Kill":{"session_name":"shell"}}),
        json!({"Preview":{"session_name":"shell"}}),
        json!({"RestoreStale":{"session_name":"shell"}}),
        json!({"DeleteStale":{"session_name":"shell"}}),
        json!("ListWorkspaces"),
        json!({"CreateWorkspace":{"name":"project"}}),
        json!({"RenameWorkspace":{"old_name":"project","new_name":"renamed"}}),
        json!({"DeleteWorkspace":{"name":"project"}}),
        json!({"SwitchWorkspace":{"name":"project"}}),
        json!({"MoveSessionToWorkspace":{"session_name":"shell","workspace_name":"project"}}),
        json!({"SaveWorkspaceState":{"workspace_name":"project","last_selected_session":"shell","last_focused_pane":"terminal","sidebar_scroll_offset":2}}),
    ] {
        round_trip::<ClientMessage>(fixture);
    }
}

#[test]
fn legacy_server_protocol_remains_compatible() {
    let window = json!({"name":"shell","is_attached":true,"rows":24,"cols":80,"last_active":1,"workspace_name":"project"});
    let session = json!({"name":"project","last_selected_session":"shell","last_focused_pane":"terminal","sidebar_scroll_offset":2});
    for fixture in [
        json!({"Sessions":{"names":[window.clone()]}}),
        json!({"Workspaces":{"workspaces":[session],"active_workspace":"project"}}),
        json!({"WorkspaceSwitched":{"name":"project","sessions":[window],"last_selected_session":"shell","last_focused_pane":"terminal","sidebar_scroll_offset":2}}),
        json!({"SessionMoved":{"session_name":"shell","workspace_name":"project"}}),
        json!({"WorkspaceCreated":{"name":"project"}}),
        json!({"WorkspaceRenamed":{"old_name":"project","new_name":"renamed"}}),
        json!({"WorkspaceDeleted":{"name":"project"}}),
        json!("WorkspaceStateSaved"),
        json!({"Attached":{"session_name":"shell","is_new":false,"terminal_state":null}}),
    ] {
        round_trip::<ServerResponse>(fixture);
    }
}

#[test]
fn legacy_metadata_keeps_session_membership_and_selection() {
    let window = json!({"name":"shell","cwd":null,"rows":24,"cols":80,"created_at":1,"last_active":2,"workspace_name":"project"});
    let decoded: WindowMetadata = serde_json::from_value(window.clone()).unwrap();
    assert_eq!(decoded.session_name, "project");
    round_trip::<WindowMetadata>(window);
    let session = json!({"name":"project","created_at":1,"last_selected_session":"shell","last_focused_pane":"sidebar","sidebar_scroll_offset":2});
    let decoded: SessionMetadata = serde_json::from_value(session.clone()).unwrap();
    assert_eq!(decoded.last_selected_window.as_deref(), Some("shell"));
    round_trip::<SessionMetadata>(session);
    let old: WindowMetadata = serde_json::from_value(
        json!({"name":"shell","cwd":null,"rows":24,"cols":80,"created_at":1,"last_active":2}),
    )
    .unwrap();
    assert_eq!(old.session_name, "Default");
    assert!(SessionMetadata::file_path().ends_with("workspaces.json"));
}

#[test]
fn cli_help_uses_tmux_terms_and_accepts_legacy_aliases() {
    let binary = env!("CARGO_BIN_EXE_sb");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for term in [
        "list-windows",
        "kill-window",
        "session",
        "server",
        "--window",
    ] {
        assert!(text.contains(term), "Missing {term}: {text}");
    }
    assert!(!text.contains("workspace"));
    assert!(!text.contains("daemon"));
    for args in [
        vec!["workspace", "list", "--help"],
        vec!["session", "kill", "--help"],
        vec!["workspace", "delete", "--help"],
        vec!["daemon", "--help"],
        vec!["list", "--help"],
        vec!["kill", "--help"],
        vec!["--session", "shell", "--help"],
        vec!["-s", "shell", "--help"],
        vec!["--window", "shell", "--help"],
    ] {
        assert!(
            Command::new(binary)
                .args(&args)
                .output()
                .unwrap()
                .status
                .success(),
            "Rejected {args:?}"
        );
    }
}
