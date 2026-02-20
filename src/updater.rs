/// Self-update mechanism for sidebar-tui.
///
/// Usage:
/// - `check_and_notify()` — call on startup to check for updates (once per day, cached)
/// - `run_self_update()` — call when user runs `sb self-update`

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

const GITHUB_OWNER: &str = "ftl-tools";
const GITHUB_REPO: &str = "sidebar_tui";
const CACHE_FILE_NAME: &str = "update-check.json";
const CHECK_INTERVAL: Duration = Duration::from_secs(86400); // 24 hours

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    last_checked_secs: u64,
    latest_version: String,
}

/// Returns true if the binary is installed via Homebrew.
pub fn is_homebrew_install() -> bool {
    let homebrew_prefix = std::env::var("HOMEBREW_PREFIX").unwrap_or_default();
    if let Ok(exe) = std::env::current_exe() {
        let path_str = exe.to_string_lossy().to_lowercase();
        if path_str.contains("/homebrew/")
            || path_str.contains("/linuxbrew/")
            || (!homebrew_prefix.is_empty() && path_str.starts_with(&homebrew_prefix.to_lowercase()))
        {
            return true;
        }
    }
    false
}

fn cache_file_path() -> Option<PathBuf> {
    let cache_dir = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".cache"));
    Some(cache_dir.join("sidebar-tui").join(CACHE_FILE_NAME))
}

fn read_cache() -> Option<CacheEntry> {
    let path = cache_file_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(entry: &CacheEntry) {
    if let Some(path) = cache_file_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string(entry) {
            let _ = std::fs::write(path, data);
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Fetches the latest version from GitHub Releases API.
/// Returns None on network error or parse failure.
fn fetch_latest_version() -> Option<String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        GITHUB_OWNER, GITHUB_REPO
    );

    let response = ureq::get(&url)
        .set("User-Agent", &format!("sb/{}", env!("CARGO_PKG_VERSION")))
        .call()
        .ok()?;

    let json: serde_json::Value = response.into_json().ok()?;
    let tag = json["tag_name"].as_str()?;
    // Strip leading 'v' if present
    Some(tag.trim_start_matches('v').to_string())
}

/// Returns the latest version if one is available and the cache is fresh enough.
/// Fetches from GitHub if the cache is stale (> 24h old). Returns None on any error.
pub fn get_latest_version_cached() -> Option<String> {
    let current_secs = now_secs();

    if let Some(cache) = read_cache() {
        let elapsed = current_secs.saturating_sub(cache.last_checked_secs);
        if elapsed < CHECK_INTERVAL.as_secs() {
            return Some(cache.latest_version);
        }
    }

    // Cache is stale or missing — fetch from GitHub
    let latest = fetch_latest_version()?;
    write_cache(&CacheEntry {
        last_checked_secs: current_secs,
        latest_version: latest.clone(),
    });
    Some(latest)
}

/// Check if a version string is newer than the current binary version.
fn is_newer(latest: &str) -> bool {
    let parse = |v: &str| -> (u64, u64, u64) {
        let parts: Vec<&str> = v.split('.').collect();
        let n = |i: usize| parts.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
        (n(0), n(1), n(2))
    };
    parse(latest) > parse(env!("CARGO_PKG_VERSION"))
}

/// Check for updates on startup and print a notification if a newer version is available.
/// Does nothing if:
/// - The cache says we checked within the last 24h and no update is available
/// - Network is unavailable
pub fn check_and_notify() {
    // Run check in a background thread to avoid blocking startup
    std::thread::spawn(|| {
        let Some(latest) = get_latest_version_cached() else { return };
        if !is_newer(&latest) {
            return;
        }

        if is_homebrew_install() {
            eprintln!("sb v{} available — run 'brew upgrade sb' to update", latest);
        } else {
            eprintln!(
                "sb v{} available — run 'sb self-update' to update",
                latest
            );
        }
    });
}

/// Perform a self-update by downloading the latest release from GitHub and replacing the binary.
/// Prints progress to stdout. Exits with an error message if Homebrew-installed.
pub fn run_self_update() -> color_eyre::Result<()> {
    if is_homebrew_install() {
        let latest = get_latest_version_cached();
        if let Some(v) = latest {
            if is_newer(&v) {
                println!("sb v{} available — run 'brew upgrade sb' to update", v);
            } else {
                println!("Already up to date.");
            }
        } else {
            println!("sb is managed by Homebrew. Run 'brew upgrade sb' to update.");
        }
        return Ok(());
    }

    println!(
        "Checking for updates (current: v{})...",
        env!("CARGO_PKG_VERSION")
    );

    let status = self_update::backends::github::Update::configure()
        .repo_owner(GITHUB_OWNER)
        .repo_name(GITHUB_REPO)
        .bin_name("sb")
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(self_update::cargo_crate_version!())
        .build()?
        .update()?;

    match status {
        self_update::Status::UpToDate(v) => println!("Already up to date (v{}).", v),
        self_update::Status::Updated(v) => println!("Updated to v{}.", v),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_detects_higher_version() {
        assert!(is_newer("999.0.0"));
    }

    #[test]
    fn test_is_newer_same_version_is_false() {
        let current = env!("CARGO_PKG_VERSION");
        assert!(!is_newer(current));
    }

    #[test]
    fn test_is_newer_lower_version_is_false() {
        assert!(!is_newer("0.0.1"));
    }

    #[test]
    fn test_homebrew_detection_env_var() {
        // With HOMEBREW_PREFIX set and a matching exe path this returns true.
        // We can only test the negative case here without spawning a subprocess.
        // If not in Homebrew environment, result depends on current_exe path.
        let _ = is_homebrew_install(); // just make sure it doesn't panic
    }

    #[test]
    fn test_cache_roundtrip() {
        // Write a cache entry and read it back
        let entry = CacheEntry {
            last_checked_secs: 12345,
            latest_version: "1.2.3".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: CacheEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.last_checked_secs, 12345);
        assert_eq!(decoded.latest_version, "1.2.3");
    }
}
