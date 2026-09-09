# Testing and diagnostics

Use the bounded runner for development:

```bash
npm test
# Without Node/npm:
python3 -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/test_runner.py
```

Requires Python 3.9+, Cargo, and macOS/Linux (WSL on Windows). Native tmux tests require the version supported by the preview (currently 3.6a); missing tmux is a failure, not a silent skip.

## Explicit test tiers

| Command | Scope |
| --- | --- |
| `npm test` | Runner self-tests, all library/binary/compatibility/scaffold tests, all tmux integrations, current legacy terminology and throughput workflows, harness regressions |
| `npm run test:unit` | Library, binary and terminology compatibility tests; no desktop/tmux dependency |
| `npm run test:tmux` | All isolated tmux inspector/chooser/sidebar and legacy-without-tmux tests |
| `npm run test:e2e` | **All** legacy UI scenarios, including those still asserting retired bindings/layouts |
| `npm run test:full` | All Cargo test targets; not an alias for the fast gate |
| `npm run test:desktop` | Explicit macOS Terminal.app evidence run, one worker, 60 seconds per case / 90 seconds overall |

The fast gate is intentionally **not** a claim that every old legacy UI scenario passes. No legacy tests were disabled to manufacture a green full-suite result. The existing 47 ignored unit tests remain reported as ignored, not passed.

```bash
# Focused regression, with a hard deadline per attempt:
python3 scripts/test_runner.py --suite e2e --filter test_tmux_terminology_workflow --timeout 10

# Stability, not automatic retries: every attempt must pass.
python3 scripts/test_runner.py --suite unit --filter test_process_resize_updates_all_windows --repeat 20 --timeout 3

# Cold builds or a deliberately longer scenario:
python3 scripts/test_runner.py --suite full --build-timeout 120 --timeout 45 --run-timeout 180
```

A filter matching zero cases is an error. `--verbose` prints every case; normal output shows regular progress, slow cases, failures, and running-test heartbeats. `--jobs` defaults to four. Recent timing reports schedule slow cases first, but never change which cases run.

## Why this is more reliable

- **Build once.** Cargo emits exact test executable paths with `--no-run --message-format=json --locked`. Tests no longer recursively invoke `cargo build` or assume `target/debug/sb`.
- **One process per case.** Environment mutations, crashes and PTY-spawning tests cannot strand an entire in-process libtest suite. Each process has a private HOME/XDG/resource root and a controlled `/bin/sh`, with no personal shell startup hooks.
- **Hard deadlines.** Default: 30 seconds per case and **60 seconds for the entire run**, including build/discovery. A separate build cap defaults to 60 seconds but cannot extend the overall budget. The watchdog fails the case and stops its process group. Overall expiry cancels queued/running work and exits **124** with a partial report when cases have been discovered. Ctrl+C/SIGTERM also cancels work; no implicit retry conceals a failure.
- **No inherited-pipe deadlock.** Output goes directly to a log file. A daemon inheriting stdout cannot keep a completed command waiting for pipe EOF.
- **Scoped recovery.** Fixtures place their sockets beneath a unique runner-owned `/tmp/sb-check-*` directory. Even after SIGKILL prevents Rust `Drop`, cleanup addresses only sockets inside that directory. Detached tmux servers use explicit `-N -S PATH kill-server`; legacy servers use their private XDG socket. Failed cleanup fails the run and retains the resource directory. Arbitrary external processes that escape process groups without registering owned sockets are not claimed to be recoverable.
- **Readiness instead of delay.** Shared terminal drains settle after 20 ms idle, cap a busy drain at 200 ms, and check deadlines *inside* continuous-output loops. Text assertions have a single wall-clock budget. Session creation waits for committed IPC state, not a matching draft string.
- **Real assertions.** The environment inheritance test no longer mutates the parallel parent process or passes on echoed commands. The throughput test executes 1,000 deterministic shell-output lines, replacing a `bd list` benchmark that depended on local tools and touched the real repository database.

### Agent tool backstop

This Bash tool's `timeout` is in **seconds**. The old `600000` instruction was a milliseconds/seconds mistake: it allowed almost seven days. It is no longer a requirement.

Use **`timeout: 90`** for normal commands around the runner's 60-second overall budget. Explicit full-audit npm scripts request 180 seconds; use **`timeout: 210`** around those. Desktop's 90-second overall budget needs a 120-second tool backstop. Scoped cleanup is limited to five seconds per active worker, plus bounded process reaping. A timeout is a failure to inspect, not permission to silently increase budgets. The separate tool deadline protects against the runner itself becoming stuck.

The whole-run limit matters even when each individual case has a timeout: hundreds of individually stalled cases must not accumulate into hours. Regression tests simulate a stalled build and ten queued/running cases with a 0.3-second overall budget, verifying prompt failure and cancellation without waiting through every case.

Reports are saved under `target/test_reports/<run>/`: build/discovery logs, one output log per case, cleanup diagnostics, and `results.json` with statuses and durations. CI uploads the same reports. Tests that fail, time out, cannot be discovered, execute zero cases, or fail cleanup all produce a nonzero exit status.

## Measured handoff

macOS 15.8, tmux 3.6a; development measurements, not cross-platform performance promises:

| Check | Evidence |
| --- | --- |
| Original current legacy terminology workflow | 12.12 seconds in the preceding installed-CLI validation |
| Readiness-based terminology workflow | 10/10 attempts passed, 2.04–2.57 seconds each (four workers) |
| Previously hung resize unit case | 20/20 isolated attempts passed, approximately 0.01–0.03 seconds each |
| Runner regression tests | 10 passed in 0.43 seconds, including timeout, cancellation, inherited stdout, unrelated-process preservation, and socket-cleanup boundaries |
| Fast gate after refactor | 452 passed, 47 existing ignored, 0 failed; 13.81 seconds including build/discovery on the final `npm test` run (earlier incremental rebuild run: 18.66 seconds) |
| Forced native fixture timeout | 0.5-second watchdog deliberately fired; case failed as `TIMEOUT`, private tmux socket/resources cleaned, no new resource directory left behind |
| Direct `cargo test --lib --locked` | 358 passed, 47 ignored; 1.07 seconds execution (10.57 seconds including rebuild), protected by a 20-second outer watchdog |
| Full legacy audit during refactor | 92 cases in 82.84 seconds at eight workers: 38 passed, 52 assertion failures, 2 hard timeouts; not a passing release gate |

The interrupted unit log identified `server::tests::test_process_resize_updates_all_windows` running beyond 60 seconds. No stack sample was obtained, so the exact low-level cause is not claimed fixed. Removing unsafe parallel environment mutation and adding process isolation/watchdogs prevents the same symptom from silently blocking the development gate.

### Remaining legacy audit work

Many audit failures explicitly expect retired Ctrl+W/Ctrl+N/Ctrl+Q/Ctrl+Z bindings, old hint text, or a session header on the second row. Others still need individual triage; **not all 54 failures have been proven pre-existing**. The two 30-second cutoffs were the sidebar-scroll restoration and overflowing-window-list scenarios. Their logs are in the audit report `20260909_071922_52161`; later shared-drain refinements and the deterministic throughput replacement were validated by focused/harness/fast tests, not another full audit.

This is a faster, bounded test system—not completion of the outstanding legacy assertion migration. Update those flows against the current interaction contract instead of restoring long blanket sleeps or marking failures ignored. A future release review must run the full tier and resolve its failures.

## Installation is separate

After a functioning code batch:

```bash
cargo install --path . --force --locked --offline
```

With cached dependencies this avoids unnecessary registry updates and dependency re-resolution. If a dependency is not cached, retry explicitly without `--offline`. Do not repeatedly combine test/build/install/desktop checks into one opaque command. The installed CLI remains version 0.1.17; this work does not advance the [tmux architecture acceptance gate](./tmux_migration_progress).
