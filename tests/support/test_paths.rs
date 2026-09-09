//! Runner-owned resource roots let the watchdog clean up even when Rust Drop
//! cannot run (timeout/SIGKILL). Plain cargo test retains private /tmp fixtures.
pub fn private_dir(prefix: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SB_TEST_RESOURCE_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/tmp".into());
    root.join(format!("{prefix}-{:016x}", rand::random::<u64>()))
}
