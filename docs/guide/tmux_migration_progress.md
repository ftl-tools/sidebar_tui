# tmux migration progress

## Step 1 — read-only inspector

**Status:** Implementation, automated gates, and installed-CLI acceptance complete. Step 1 is the only implemented migration step. GitHub synchronization is recorded by the step-numbered commit containing this handoff; verify `HEAD == origin/main` after fetching before continuing.

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
