use std::process::Command;

fn get_binary_path() -> String {
    // Use Cargo's actual artifact, including custom target directories.
    env!("CARGO_BIN_EXE_sb").to_string()
}

#[test]
fn test_binary_exists_and_is_executable() {
    let path = get_binary_path();
    assert!(
        std::path::Path::new(&path).exists(),
        "Binary should exist at: {}. Run 'cargo build' first.",
        path
    );
}

#[test]
fn test_cli_version_matches_package() {
    // Cargo already built this artifact before running tests. Rebuilding inside
    // a test added lock/network waits and tested no additional product behavior.
    let output = Command::new(get_binary_path())
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
}
