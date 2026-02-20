# Research Notes: Self-Update Mechanism for sidebar_tui

## Goal (from issue sidebar_tui-baj8)

Implement the spec's self-update feature:
> "Binary checks GitHub Releases API once per day (cached) and self-updates on next run"

With special handling:
> "If installed via Homebrew, skip self-update and print: `sb v0.2.0 available — run brew upgrade sb to update`"

---

## Key Finding: Two Different Crates Serve Different Needs

There are two distinct needs:
1. **Check if an update is available** (version check with caching) — use `update-informer`
2. **Actually download and replace the binary** (self-update) — use `self_update`

These can be combined: use `update-informer` for periodic checking, then `self_update` to perform the actual update.

---

## Crate 1: `update-informer` (Version Checking + Caching)

**Source:** `references/self-update/update-informer/`
**GitHub:** https://github.com/mgrachev/update-informer
**Version:** 1.1+

### What it does
- Checks GitHub Releases API (or Crates.io, npm, PyPI) for newer versions
- Caches the result to a file in the system cache directory (XDG-compliant via `etcetera` crate)
- Default interval: **24 hours** — first check only happens after the interval expires
- Does NOT download or replace the binary — it only tells you a new version exists

### Usage pattern
```rust
use update_informer::{registry, Check};

let name = "owner/repo";
let version = env!("CARGO_PKG_VERSION");
let informer = update_informer::new(registry::GitHub, name, version)
    .interval(Duration::from_secs(60 * 60 * 24)); // 24 hours (already the default)

if let Some(new_version) = informer.check_version().ok().flatten() {
    println!("sb {} available — run `sb self-update` to update", new_version);
}
```

### Cargo.toml
```toml
[dependencies]
update-informer = { version = "1.1", default-features = false, features = ["github", "reqwest", "rustls-tls"] }
```

### Cache mechanism (from `src/version_file.rs`)
- Cache file location: `{cache_dir}/update-informer-rs/{registry}-{pkg_name}`
- Uses `etcetera` crate which respects `XDG_CACHE_HOME` on Linux, `~/Library/Caches` on macOS
- Cache check: compares file mtime (last-modified timestamp) against the interval duration
- On first run: writes current version to cache file, returns no update (waits one interval)
- After interval: checks GitHub API, writes new version to cache, returns it if newer

### XDG Cache directory resolution (via `etcetera`)
```rust
use etcetera::BaseStrategy;
let base_dir = etcetera::choose_base_strategy()?;
let cache_dir = base_dir.cache_dir().join("sidebar-tui");
// macOS: ~/Library/Caches/sidebar-tui/
// Linux: ~/.cache/sidebar-tui/ (or $XDG_CACHE_HOME/sidebar-tui/)
```

For `sidebar_tui`, you could do your own XDG cache without `etcetera` using:
```rust
let cache_dir = std::env::var("XDG_CACHE_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| {
        dirs::home_dir().unwrap().join(".cache")
    })
    .join("sidebar-tui");
```

### Testing support
```rust
#[cfg(test)]
let informer = update_informer::fake(registry::GitHub, name, version, "1.0.0");
```

---

## Crate 2: `self_update` (Binary Download + Replacement)

**Source:** `references/self-update/self_update/`
**GitHub:** https://github.com/jaemk/self_update
**Version:** 0.42.0

### What it does
- Downloads a release asset from GitHub Releases
- Extracts the binary from the archive (tar.gz or zip)
- Replaces the running binary in-place using `self_replace`
- Handles platform target detection via `get_target()` (e.g., `x86_64-apple-darwin`)

### Usage pattern
```rust
use self_update::cargo_crate_version;

fn do_self_update() -> Result<(), Box<dyn std::error::Error>> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("your-org")
        .repo_name("sidebar_tui")
        .bin_name("sb")
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    match status {
        self_update::Status::UpToDate(v) => println!("Already up to date (v{})", v),
        self_update::Status::Updated(v) => println!("Updated to v{}", v),
    }
    Ok(())
}
```

### Cargo.toml
```toml
[dependencies]
self_update = { version = "0.42", features = ["archive-tar", "compression-flate2", "rustls"], default-features = false }
```

### How binary replacement works
1. Downloads asset matching `get_target()` string (e.g., `sb-v1.2.3-x86_64-apple-darwin.tar.gz`)
2. Extracts the binary to a temp directory
3. Calls `self_replace::self_replace(new_exe)` which:
   - On Unix: renames old binary, moves new binary into place
   - On Windows: uses special techniques to replace a running binary

### Asset naming convention for GitHub Releases
The binary must be uploaded as `sb-{version}-{target}.tar.gz`, e.g.:
- `sb-v1.2.3-x86_64-apple-darwin.tar.gz`
- `sb-v1.2.3-aarch64-apple-darwin.tar.gz`
- `sb-v1.2.3-x86_64-unknown-linux-gnu.tar.gz`
- `sb-v1.2.3-x86_64-pc-windows-msvc.zip`

---

## Homebrew Detection Pattern

### How to detect if installed via Homebrew

The binary's path when installed via Homebrew will contain Homebrew's prefix:
- macOS Apple Silicon: `/opt/homebrew/Cellar/sb/...` or `/opt/homebrew/bin/sb`
- macOS Intel: `/usr/local/Cellar/sb/...` or `/usr/local/bin/sb`
- Linux Homebrew: `/home/linuxbrew/.linuxbrew/...`

Detection code:
```rust
fn is_installed_via_homebrew() -> bool {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return false,
    };

    let path_str = exe.to_string_lossy();

    // Check known Homebrew paths
    path_str.contains("/homebrew/")
        || path_str.contains("/Homebrew/")
        || path_str.contains("/linuxbrew/")
        // Also check HOMEBREW_PREFIX env var if set
        || std::env::var("HOMEBREW_PREFIX")
            .map(|prefix| path_str.starts_with(&prefix))
            .unwrap_or(false)
}
```

### Why skip self-update for Homebrew installs
- Homebrew manages the binary's location and permissions — self-replacing would break Homebrew's tracking
- `brew upgrade sb` is the correct update mechanism for Homebrew users
- Same pattern used by many CLIs: Homebrew-installed copies should be updated via Homebrew

---

## Recommended Implementation Strategy for sidebar_tui

### Approach: `update-informer` for version checking + `self_update` for actual update

```
sb startup:
  1. Check Homebrew detection
  2. If Homebrew: skip update, but check version cache
     - If new version available: print "sb vX.Y.Z available — run brew upgrade sb to update"
  3. If NOT Homebrew:
     - Use update-informer (24h cache) to check GitHub
     - If new version available: print "sb vX.Y.Z available — run sb self-update to update"

sb self-update subcommand:
  1. Check Homebrew: if Homebrew, print message and exit
  2. Run self_update::backends::github::Update to download + replace binary
```

### Cache file location (DIY without update-informer)
If building manually:
```rust
// Cache file: $XDG_CACHE_HOME/sidebar-tui/update-check
// Contents: JSON { "last_checked": <unix_timestamp>, "latest_version": "1.2.3" }
let cache_dir = std::env::var("XDG_CACHE_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".cache"))
    .join("sidebar-tui");
std::fs::create_dir_all(&cache_dir)?;
let cache_file = cache_dir.join("update-check");

// Check if 24h have passed since last check
if let Ok(metadata) = std::fs::metadata(&cache_file) {
    let elapsed = metadata.modified()?.elapsed().unwrap_or_default();
    if elapsed < Duration::from_secs(86400) {
        // Read cached version from file
        return Ok(std::fs::read_to_string(&cache_file)?);
    }
}

// Do the actual API call, write result to file
```

---

## Reference Files in This Directory

- `self_update/` — Full source of jaemk/self_update crate (v0.42.0)
  - `src/update.rs` — Core update logic, shows how binary replacement works
  - `src/backends/github.rs` — GitHub releases API integration
  - `examples/github.rs` — Complete working example
- `update-informer/` — Full source of mgrachev/update-informer crate (v1.1+)
  - `src/lib.rs` — Main API, shows 24h interval default and caching logic
  - `src/version_file.rs` — Cache file implementation using file mtime
  - `src/registry/github.rs` — GitHub releases API call
  - `README.md` — Full usage docs with all examples
- `etcetera/` — XDG base directory resolution library used by update-informer

---

## Nushell as Reference Implementation

Nushell (PR #14813) integrated update-informer with:
- Stable: 24h cache interval
- Nightly: 1h cache interval
- Config option `$env.config.check_for_new_version` to disable

Ultimately they moved to an explicit `version check` command (PR #14880) rather than startup notification.

**For sidebar_tui**: the spec says "once per day cached check" and "self-updates on next run" — this implies background-style checking (not user-triggered), suggesting the startup notification approach is correct.

---

## Key Decisions for Implementer

1. **Use `update-informer` + `self_update` together** — update-informer handles the daily check with caching, self_update handles the actual binary replacement.

2. **Homebrew check FIRST** — before any self-update logic, check `current_exe()` path.

3. **GitHub releases asset naming** — must match the pattern `sb-{version}-{target}.{ext}` where target comes from `self_update::get_target()`.

4. **`no_confirm(true)`** — for automatic updates (the spec says "self-updates on next run"), skip interactive confirmation.

5. **Cross-platform rustls** — use `features = ["rustls"]` and `default-features = false` to avoid OpenSSL dependency issues in cross-compilation.
