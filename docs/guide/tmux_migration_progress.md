# tmux migration progress

## Current handoff — Step 3 reviewed; human acceptance still required

Reviewed on macOS 15.8 with tmux 3.6a against `389b78a` on `main` (matched fetched `origin/main`). The Step 3 implementation in `src/tmux_sidebar.rs` and all six tests in `tests/tmux_sidebar.rs` are present. No missing implementation was identified within the restricted disposable-server contract, and no production code was changed. **Step 3 is still not complete:** physical-terminal mouse/keyboard validation and explicit acceptance of shared focus/layout remain human gates, not something a passing automated test can approve. Step 4 was not started.

### Fresh validation

All piped checks captured both streams with `set -o pipefail` and `2>&1 | tee`; normal outer timeouts were 90 seconds, full E2E 210 seconds, desktop 120 seconds. No failed case was retried or suppressed.

| Command | Result |
| --- | --- |
| `npm test` | 12 Python runner tests passed; 452 Cargo cases passed, 47 existing ignored, 0 failed; 17.71 seconds. Report `target/test_reports/20260909_081920_98861/`. |
| `python3 scripts/test_runner.py --suite e2e --run-timeout 180` | 39 passed, 52 assertion failures, 2 per-case timeouts; 121.03 seconds. Report `target/test_reports/20260909_081948_6291/`. Not a passing full legacy gate. |
| `cargo install --path . --force --locked --offline` | Reinstalled `/Users/melchiahmauck/.cargo/bin/sb`, version 0.1.17. Existing unused `IpcListener` and locked yanked WASM dependency warnings; no dependency/version changes. |
| `SB_TMUX_TEST_BINARY="$HOME/.cargo/bin/sb" cargo test --locked --test tmux_sidebar --test tmux_chooser --test tmux_inspector --test tmux_legacy_launch` | All 15 passed using the installed CLI: 6 sidebar, 6 chooser, 2 inspector, 1 legacy-without-tmux. Log `/tmp/sb_step3_review_installed.txt`. Direct Cargo invocation used because the isolated runner deliberately strips the binary override. |
| `npm run test:desktop` | 1 passed, 14.86 seconds overall. Report `target/test_reports/20260909_082209_8929/`. This run uses the built debug CLI, not the installed override. |
| `npm run docs:build` | Passed, VitePress 1.6.4. |

Full-audit timeout logs `0072_e2e.log` and `0079_e2e.log` stop during isolated-server setup for `test_sidebar_scroll_position_restored_on_session_switch` and `test_truncation_indicators_when_window_list_overflows`. The inspected Ctrl+N failure still asserts a retired binding. These are unresolved legacy audit issues; this review does not classify every failure as a stale assertion or claim full legacy correctness.

### Fresh desktop evidence and decision boundary

The optional helper created and validated its own Terminal.app window 15507, TTY `/dev/ttys066`. Evidence remains local at `/var/folders/3c/5k5l4l7x00g60styy_w4m9fh0000gn/T/sb-step3-evidence-c633581175c4d66c/`. The reviewer opened `01_editor.png`, `03_mouse_selection.png`, and `04a_terminal_resized.png`: they show saved editor content, highlighted native copy-mode text, and usable hints beside continuing logs after resize to 70×20 (the list title clips). This is agent-inspected rendering evidence, **not physical mouse input or user sign-off**. Input came through a second native PTY client, with synthetic SGR mouse reports.

Exact assertions/output: keyboard copy `STEP3_LOG_000039`, mouse copy `STEP3_LOG_000038`, history offset 0 → 5, native work-pane geometry `[29, 0, 51]` → `[33, 0, 47]`. Original workers remained `@0 %0 9258` and `@1 %1 9271` after close/reopen; saved editor text remained unchanged and log bytes advanced 1581 → 1649. Two attached clients shared window/pane selection. The owned test server/window were cleaned up by the fixture; no default/legacy user server was targeted.

**Recommendation:** retain the provisional one-left-pane-per-window design for this experimental scope, subject to the user's acceptance. Remaining sign-off is the [manual demo checklist](./tmux_sidebar_demo#required-real-terminal-acceptance-please-report-evidence), especially physical mouse/scroll/copy and whether shared focus/width plus unzoom-on-entry are acceptable. Do not interpret the request to investigate/finish Step 3 as evidence that those observations or approval already happened. After acceptance, record the decision and re-estimate Steps 4–11 before continuing. Existing-server integration, linked-window lifecycle, notification recovery, and the release/deprecation interval remain substantial separate work.

Unrelated `.beads/beads.db-shm`, `.beads/beads.db-wal`, and untracked `docs/temp.md` were present at review start and are excluded from this task's commit.

## Previous handoff — migration paused for test-harness overhaul

**Knowledge workflow resolved:** Mulch is no longer required or recommended for agent startup/completion. The project chose checked-in Markdown handoffs rather than installing the missing CLI. `AGENTS.md` documents the replacement workflow; existing `.mulch` records remain an unchanged historical archive. Earlier command-not-found entries below are history, not instructions to retry those commands.

**Timeout correction:** numerical `600000` Bash timeout records below are historical, not current instructions. This tool uses seconds. The runner now defaults to a 60-second overall budget with a 90-second outer tool timeout; see [testing](./testing#agent-tool-backstop). Full reviews explicitly request 180 seconds (210-second tool backstop).

The user redirected this session from Step 3 acceptance to **making tests faster and reliable** after a unit test hung. Do not interpret the new test tooling or successful fast gate as Step 3 architecture acceptance. No Step 4 implementation was started.

- Added a real editor/logging regression to `tests/tmux_sidebar.rs`: saved `vi` content, continuous numbered logs, keyboard copy, SGR mouse focus/history/selection/copy, native border resizing, zoom/reentry, and unchanged working IDs/PIDs after close/reopen. The sidebar integration target now has six tests. It passes in the bounded fast gate.
- Optional `tests/support/tmux_desktop.rs` opens a new, TTY-validated Terminal.app window for visual evidence. Earlier agent-driven runs captured actual editor, zoom/unzoom, shared focus, mouse-selection and resize rendering. Input was driven through a second native PTY client, **not** physical OS mouse events; macOS Accessibility control was unavailable. Screenshots remain local temporary artifacts. No human approval or final architecture acceptance was recorded.
- Early desktop automation incorrectly assumed Terminal's front window was the newly created window; in that run it captured/closed another agent-created demo window. The helper now resolves a newly created window by its exact TTY, validates sole-tab ownership before captures/cleanup, and waits for that TTY to attach. Do not use the early captures as acceptance evidence.
- The interrupted unit log ended at `test_process_resize_updates_all_windows` running over 60 seconds. The new runner isolates cases and has hard watchdogs, heartbeats, scoped socket cleanup, and persistent timing/failure reports. See [testing and diagnostics](./testing) for exact commands, measured speedups, and the failing full legacy audit—not just the green fast subset.
- The legacy audit exposed a `bd list` performance test accessing this repository's actual `.beads` database. It is replaced by a deterministic shell-throughput test. SQLite WAL/SHM changes were not blindly restored because those runtime files may be in use by other clients.
- CLI rebuilt/reinstalled with `cargo install --path . --force --locked --offline`; version unchanged. Existing legacy data/default servers were not intentionally stopped. The unfinished Step 3 gate below remains the next migration task after the test-system work.

## Step 3 — native sidebar proof (INCOMPLETE / waiting for architecture gate)

**Status:** Working prototype and automated gates delivered as an explicitly incomplete checkpoint. **Do not check off Step 3 or advance to Step 4.** Real-terminal editor/logging, native mouse/copy-mode evidence, human acceptance of shared focus/layout, and the post-decision re-estimate are still outstanding. Exact instructions and an evidence checklist are in the [native sidebar demo guide](./tmux_sidebar_demo#required-real-terminal-acceptance-please-report-evidence).

### Prerequisites / scope

Fetched `origin` on clean `main`; local and remote matched completed Step 2 commit `926adf6d99d164ee66b015a91d8250ea98eab589`. Read AGENTS, the full plan, prior handoff, and existing sidebar/hint conventions. No `refs/` directory exists. Steps 1–2 remain functional; no legacy routing, server/data deletion, version bump, or release deployment.

### Implemented architecture under evaluation

- New explicit `sidebar-demo`, `sidebar-show`, and `sidebar-close` preview commands; internal `sidebar-pane` is launched directly by tmux. Require an explicitly supplied absolute socket path in an owned private 0700 directory. Refuse unowned existing servers; create a fresh dedicated server using `/dev/null` config, with one marked session and two marked working windows.
- One native left split per enrolled window, initially 28 columns. Marked pane roles and remembered original working IDs use namespaced tmux user options. No titles/names used as ownership substitutes. Native applications remain rendered by tmux. The existing metadata-list styling and HintBar render inside the real pane dimensions.
- Native conditional root-F12 binding is opt-in, collision checked, and removed/replaced only when its canonical tmux serialization still matches the saved owned binding. Exact-key queries handle repeat (`-r`) bindings as well. No default prefix replacement, file configuration write, or background focus launcher.
- Enter focuses the destination sidebar **before** switching its window; Tab/Esc returns to the one original working pane. Native select-pane without -Z defines unzoom-on-entry. q/Ctrl+C terminates only the current sidebar; explicit show recreates missing display panes. F12 is not a supervisor after close/crash: reentry requires show to refresh the binding targets.
- A kernel flock in the private directory serializes concurrent enrollment/show/close and snapshot reads during pending splits. A crashed owner releases the kernel lock. Retain the lock file to prevent distinct-inode locking races. Pane startup waits for its ownership marker and serialized enrollment completion.
- Fresh generation/role/work-membership checks precede close/focus. The stored Sidebar process PID must also match: a native respawn can leave a stale role marker, and replacement workloads must not be killed as Sidebar. External working splits, linked/multiple sessions, missing original working panes, or duplicate marked panels are rejected instead of repaired by guessing. Native-created unmarked windows are not enrolled. General existing-server adoption remains Step 6 work.

### Validation (automated evidence, not manual acceptance)

Installed binary: `/Users/melchiahmauck/.cargo/bin/sb`; macOS, tmux 3.6a. Fixture servers use unique private sockets/config/environment; only their own servers/processes are cleaned up. All PTY/E2E calls used timeout 600000 and `set -o pipefail` with `2>&1 | tee`.

| Command | Result |
| --- | --- |
| `cargo test --lib 2>&1 \| tee /tmp/sb_step3_unit.txt` | 358 passed, 47 pre-existing ignored, 0 failed |
| `cargo test --test tmux_sidebar 2>&1 \| tee /tmp/sb_step3_sidebar.txt` | 5 passed, none skipped |
| `cargo install --path . --force` | Installed each functioning production batch; version remains 0.1.17 |
| `SB_TMUX_TEST_BINARY=/Users/melchiahmauck/.cargo/bin/sb cargo test --test tmux_sidebar --test tmux_chooser --test tmux_inspector --test tmux_legacy_launch 2>&1 \| tee /tmp/sb_step3_installed.txt` | 14 passed; CLI and spawned native sidebar processes use the installed binary |
| `cargo test --test e2e test_tmux_terminology_workflow -- --nocapture 2>&1 \| tee /tmp/sb_step3_legacy.txt` | 1 passed, 88 filtered out |
| `cargo test --bin sb --test terminology_tests 2>&1 \| tee /tmp/sb_step3_cli.txt` | 67 binary unit tests and 4 compatibility tests passed |
| `npm run docs:build` | Passed, VitePress 1.6.4 |

Installed structural evidence: three simultaneous launches and four concurrent shows retain exactly two Sidebar panes; original working IDs/PIDs survive repeated show, a fixture-owned Sidebar-process SIGKILL, reopen, close-all/disable-binding, and another reopen. A real two-client tmux PTY test asserts shared window/pane selection, native resize and unzoom-on-F12, sidebar rendering after resize, and copy-mode entry/cancel. Raw working `cat` processes received **zero bytes** when F12 was immediately followed by navigation/commit and unsupported keys. Binding tests preserve both initial repeat-key collisions and later user edits; unowned servers and implicit socket authorization are rejected. The standalone chooser/inspector and legacy-without-tmux acceptance tests remain passing.

### Decisions, limitations, waiting requirements

- **Provisional strategy:** pane-per-window with synchronous native focus routing. Automated structural tests support feasibility, but this is **not approved** until the real-terminal workflow is accepted. If it fails UX requirements, preserve Step 2 and ask about a chooser/popup-first alternative; never substitute custom terminal rendering.
- **Observed multi-client behavior:** clients sharing the demo session see the same selected window and pane. Width/layout are shared tmux state, not client-private; initial 28-column width is not locked across native resizes. The tests do not establish general compatibility with existing plugins or user configs.
- **Outstanding manual evidence:** run the guide's editor and continuously printing workloads in a real terminal; demonstrate native copy mode and mouse selection/scrolling, resize/zoom/reentry, close/reopen and workload survival, plus the two-client workflow. Report terminal/OS/version, observed results, and pane/PID output. No such result or approval has been invented.
- **Post-gate re-estimate pending:** there are eight remaining numbered stages (4–11), including two approval/release boundaries and an actual deprecation interval. The passing focus prototype reduces uncertainty around native input routing but not existing-server ownership, linked-window lifecycle, notifications, or UX parity. Final re-estimation must follow the human architecture decision; do not promise a short fixed rewrite or collapse the release interval.
- **Known prototype limits:** only the dedicated owned topology; no automatic recovery of partially created demo topology or unexpected working splits. A failed enrollment may leave already-created marked demo panes; workloads remain and errors are explicit. A narrow terminal may reject insertion, in which case the chooser remains available. These limitations must be judged at this gate, not silently claimed as general integration.
- Mulch unavailable: `mulch prime`, `mulch learn`, and `mulch sync` each returned command not found; no Mulch record was written. Findings (locking, exact binding serialization, native focus ordering) are preserved here. Existing unused `IpcListener` warning and ignored binding tests unchanged; no full release/review E2E run claimed.
- **Stop here:** request `/loop-stop` while waiting for evidence and explicit architecture acceptance. Resume a pipeline beginning with unfinished Step 3. A pushed checkpoint is not completion or authorization for Step 4.

## Step 2 — interactive chooser without layout ownership

**Status:** Completed historical Step 2 handoff (before the incomplete Step 3 prototype). Synchronize the step-numbered commit on `main` and verify fetched SHA equality before advancing.

### Deliverable and architecture

- Bare `sb tmux` opens a standalone Ratatui chooser; inspection subcommands/JSON and ordinary legacy `sb` retain their routes. Optional `--cwd` and `--client` are chooser-only.
- Reviewed `src/sidebar.rs`, `src/hint_bar.rs`, and the keyboard guide (no `refs/` exists). A metadata-only list in the sidebar module reuses colors, focus border/highlight, and the existing HintBar. The legacy AppState list is not suitable for linked memberships; typed chooser state stays separate rather than inventing a universal backend.
- Flat session/window/pane membership rows, native active indicators, arrow/j/k/n/p navigation, `/` search, `r` refresh, and cancel. Highlight is not native selection. Polling runs 750 ms after each completed snapshot; errors clear actionable state. IDs remain generation-scoped across observations. External rename preserves highlight; disappearance/restart requires explicit reselection.
- Enter freshly validates server generation, membership, and pane. Terminal raw/alternate-screen restoration precedes native selection and attach. Inside tmux, candidates must belong to a session containing the invoking pane; ambiguity requires an explicit attached client name/TTY. No nested tmux and no cross-server switching. Invoking context is generation-scoped too.
- Only confirmed `c` then `y` on an absent/empty server may start/create a session, and only with an explicit existing absolute `--cwd`. Creation rechecks emptiness and returns to browsing. Hashes in cwd are escaped against tmux format expansion. Normal tmux config/environment apply; Sidebar does not write config or run a package manager.
- No enrollment, native sidebar panes, layout management, destructive actions, terminal renderer, or notification connection added. No legacy workloads/data touched.

### Exact validation and installed-CLI acceptance

Prerequisite synchronization: fetched `origin` on clean `main`, matching Step 1 commit `688174fefc2f19ffc6078002d10f34d385dc45b8`. No earlier approval gate applies to Step 2.

All piped tests used `set -o pipefail`; PTY/E2E calls used tool timeout 600000 and `2>&1 | tee`.

| Command | Result |
| --- | --- |
| `cargo test --lib 2>&1 \| tee /tmp/sb_step2_unit.txt` | 357 passed, 47 pre-existing ignored, 0 failed |
| `cargo test --test tmux_chooser 2>&1 \| tee /tmp/sb_step2_chooser.txt` | 6 passed: 5 PTY workflows plus fresh-target adapter regression |
| `cargo test --test tmux_chooser --test tmux_inspector --test tmux_legacy_launch 2>&1 \| tee /tmp/sb_step2_integration.txt` | 9 passed |
| `cargo test --test e2e test_tmux_terminology_workflow -- --nocapture 2>&1 \| tee /tmp/sb_step2_legacy.txt` | 1 passed, 88 filtered out |
| `cargo test --bin sb --test terminology_tests 2>&1 \| tee /tmp/sb_step2_cli.txt` | 67 binary unit tests and 4 compatibility tests passed |
| `cargo install --path . --force` | Passed after each functioning production batch (three); remains version 0.1.17 |
| `SB_TMUX_TEST_BINARY=/Users/melchiahmauck/.cargo/bin/sb cargo test --test tmux_chooser --test tmux_inspector --test tmux_legacy_launch 2>&1 \| tee /tmp/sb_step2_installed.txt` | 9 passed; CLI workflows use the installed executable, not target/debug/sb |
| `npm run docs:build` | Passed, VitePress 1.6.4 |

Installed executable: `/Users/melchiahmauck/.cargo/bin/sb`; local tmux: 3.6a on macOS. Fixtures own private sockets, HOME/config, cwd, and server-start environment. Missing tmux does not skip these gates.

Installed-CLI evidence:

- Two sessions with duplicate window names: browsing leaves inventory/PIDs/focus/layout byte-identical. External rename appears through polling; deletion clears selection and Enter does not attach another row. Disposable same-socket restart shows replacement inventory, then cancellation restores terminal settings.
- A split sibling pane is selected through search by `%2`; native tmux reports it active, receives a real shell command, and prints `NATIVE_SHELL`. Default native prefix+d detaches. Reopening and Ctrl+C restore the terminal.
- Chooser launched from a real attached tmux shell refuses ambiguous selection with two clients. Both remain in the original session. Exact client-name resolution is separately verified, invalid names rejected. After detaching the second client, Enter switches the sole intended client to the selected session and a real shell prints `SWITCHED_NATIVE`; client count remains one, proving no nested attachment.
- On an absent socket, browsing and canceled creation leave it absent. Confirmed creation uses an explicit directory containing spaces, quotes, and literal `#{session_id}`; native cwd matches its canonical path. Creating again on the now-populated server is rejected.
- An injected native-attach failure still restores original `stty -g` and leaves workloads alive. PTY checks assert alternate-screen exit as well as terminal settings on return. Existing Step 1 inspector and legacy-without-tmux workflows remain passing.

### Limitations, findings, next gate

- macOS aliases `/tmp` to `/private/tmp`; cwd assertions compare canonical paths rather than incorrectly treating the native path as a mismatch.
- tmux expands `new-session -c` format strings even with safe argv. Literal hash escaping is required in addition to avoiding shell interpolation.
- Poll frequency is bounded, but synchronous command duration is not; stalled-server asynchronous recovery belongs to Step 5. Selection is sequential native tmux state, not a rollback transaction; documented attach failure may leave the explicitly selected target active. Shared session/window focus is not client-private.
- No required real-terminal manual gate exists for this standalone slice; native attach/switch workflows were exercised through real tmux in PTYs. Native mouse/copy-mode and the permanent-sidebar architecture still require actual real-terminal evidence in Step 3. This is not that evidence.
- Mulch remains unavailable (`mulch prime`, `mulch learn`, and `mulch sync`: command not found); insights are recorded here instead of claiming Mulch success. Existing unused `IpcListener` warning and 47 ignored binding tests unchanged. Full E2E suite is reserved for review/release gates.
- Step 3 may begin only after Step 2 push/fetch verification. Before Step 4, obtain explicit architecture acceptance, real-terminal mouse/copy-mode evidence, multi-client documentation, and a revised estimate. No later parity, cutover, deprecation, or retirement approval has been granted.

## Step 1 — read-only inspector

**Status:** Completed historical Step 1 handoff (before Step 2). GitHub synchronization is recorded by the step-numbered commit containing this handoff; verify `HEAD == origin/main` after fetching before continuing.

### Scope and decisions

- Stable preview route: `sb tmux [--socket-name NAME | --socket-path PATH] doctor|list-sessions|list-windows|list-panes [--json]`.
- Legacy routes unchanged. No chooser, mutations, native enrollment, backend abstraction, dependency removal, version bump, or release deployment.
- Narrow adapter in `src/tmux.rs`; typed IDs scoped by the enclosing snapshot's socket/PID/start time/fresh observation token. Window memberships are independent of deduplicated windows and panes.
- Explicit argv execution, `-N` (never start server), `-u` (preserve UTF-8), explicit length-framed formats; no shell interpolation or human-table parsing. Names/titles are escaped in both output modes.
- Current version policy deliberately admits only tested tmux 3.6a. Format parsing verifies required capabilities. No minimum version or cross-platform support claim yet.
- Snapshots are read-only multi-request observations, not atomic transactions. Server identity is checked before/after and inconsistent references are rejected. No persisted IDs, cached inventory, retries, or destructive action replay.
- Fixtures use private sockets/configs and controlled server-start environments. Only fixture-owned resources are cleaned up.

### Validation and installed acceptance

Host: macOS; `/opt/homebrew/bin/tmux` reports `tmux 3.6a`.
Installed executable: `/Users/melchiahmauck/.cargo/bin/sb`, installed with `cargo install --path . --force` after each functioning production batch (twice).

Commands/results (E2E tool timeout: 600000; output captured with `2>&1 | tee`, with `set -o pipefail`):

| Command | Result |
| --- | --- |
| `git fetch origin; git status -sb` | Clean `main`, aligned with `origin/main` before implementation; remote `https://github.com/ftl-tools/sidebar_tui.git` |
| `cargo test --lib 2>&1 \| tee /tmp/sb_step1_unit.txt` | 355 passed, 47 pre-existing ignored, 0 failed |
| `cargo test --bin sb 2>&1 \| tee /tmp/sb_step1_cli.txt` | 67 passed |
| `cargo test --test tmux_inspector` | 2 passed, none skipped |
| `cargo test --test tmux_legacy_launch 2>&1 \| tee /tmp/sb_step1_legacy_missing.txt` | 1 PTY test passed |
| `cargo test --test e2e test_tmux_terminology_workflow -- --nocapture 2>&1 \| tee /tmp/sb_step1_legacy_e2e.txt` | 1 passed, 88 filtered out; legacy create/switch/cancel deletion/detach workflow |
| `cargo install --path . --force` | Installed version remains 0.1.17; no release published |
| `SB_TMUX_TEST_BINARY=/Users/melchiahmauck/.cargo/bin/sb cargo test --test tmux_inspector --test tmux_legacy_launch 2>&1 \| tee /tmp/sb_step1_installed.txt` | 3 passed using the installed executable |
| `npm run docs:build` | Passed (VitePress 1.6.4) |

Installed-CLI structural evidence: two sessions, two distinct window IDs with duplicate adversarial names, three memberships (one linked window), and two deduplicated panes. Native inventory including IDs, PIDs, selected windows/panes, and layouts was byte-identical before/after doctor and all human/JSON list commands. External deletion was reflected on the next invocation; same-socket restart yielded a fresh server observation and replacement session. Empty retained server returned empty records. Missing socket did not create one; invalid socket file was unchanged. Conflicting selectors, missing tmux, and old tmux returned errors. Explicit socket overrode malformed inherited context; valid context routed correctly.

Installed legacy PTY evidence: with `PATH` containing no tmux, `sb tmux doctor` produced install guidance, then `sb --window legacy-without-tmux` launched, entered sidebar focus, confirmed detach, and exited. Private legacy fixture cleanup only.

### Failures discovered and resolved

- `#{q:...}` does not escape tab/newline, so delimiter parsing was rejected before implementation. Length framing handles both and Unicode safely.
- tmux clients under `LC_ALL=C` rewrite Unicode/control bytes unless `-u` is supplied; integration exposed invalid framing and the adapter now forces UTF-8.
- tmux expands new-session window-name formats. Fixtures escape hashes to test literal format syntax rather than tmux-expanded names.
- Raw PTY substring matching failed across terminal cursor-control sequences. The legacy acceptance test now parses the screen with the existing test-only vt100 approach.

### Limitations / next stage prerequisites

- Mulch unavailable (`mulch prime`: command not found). `mulch learn` and `mulch sync` also returned command not found; no record could be stored; the useful findings are preserved above, not claimed as Mulch records.
- Existing warning: unused `IpcListener` alias in `src/server.rs`; unchanged. Existing ignored legacy-binding tests remain ignored.
- No full E2E review/release suite was run: Step 1 calls for focused coverage. No real-terminal manual UI gate applies to this noninteractive slice.
- Cancellation has no mutation/terminal state to roll back in the inspector; no dialogs exist. Legacy cancel-delete coverage is exercised by the existing focused E2E.
- Step 2 can begin only after this step's normal push and fetched SHA equality check. Step 3's explicit architecture/manual acceptance, Step 8 parity acceptance, and Steps 10–11 release/retirement approvals remain entirely outstanding.
- The supplied pipeline lists Steps 1–10, while its overall goal also mentions Step 11. Retirement requires a later explicitly approved stage after the documented deprecation cycle; never treat completion of this pipeline alone as retirement authorization.
