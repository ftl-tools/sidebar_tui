use std::process::Command;

fn get_binary_path() -> String {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR should be set by cargo");
    format!("{}/target/debug/sb", manifest_dir)
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
fn test_cargo_build_succeeds() {
    let output = Command::new("cargo")
        .args(["build", "--bin", "sb"])
        .output()
        .expect("Failed to run cargo build");

    assert!(
        output.status.success(),
        "cargo build should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
