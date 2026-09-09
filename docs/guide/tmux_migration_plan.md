# Incremental tmux migration plan

**Status:** Steps 1–2 complete. Step 3 has an incomplete prototype checkpoint pending real-terminal evidence and explicit architecture acceptance; subsequent steps remain gated. See [implementation progress](./tmux_migration_progress).

**Target:** Sidebar becomes a tmux-native interface and manager, not a competing terminal multiplexer.  
**Delivery rule:** Every numbered step produces a functional, independently testable tool. No step ends with a half-replaced backend.

## 1. Outcome and architectural direction

Keep Sidebar's navigation, contextual hints, confirmations, session organization, and agent-launch workflows. Delegate shell processes, terminal display, scrollback, copy mode, and pane layouts to tmux.

The preferred architecture is a Rust/Ratatui sidebar running in a native tmux pane. Working shells and editors run in sibling panes, rendered by tmux rather than Sidebar's `vt100` renderer.

```text
sb CLI / sidebar TUI
    -> typed management actions
    -> tmux adapter (commands, snapshots, notifications)
    -> tmux server (sessions, windows, panes, shell processes)

Native tmux client -> terminal rendering and working-pane input
Sidebar preferences -> UI-only state, not process state
```

**Important limitation:** tmux has no global vertical sidebar outside its windows. A sidebar pane per enrolled window is the initial hypothesis, not a proven design. Step 3 must validate it before the migration proceeds. Running a native tmux pane also means global Sidebar shortcuts must be implemented through tmux bindings, not the sidebar's local input loop.

### Non-goals for the migration

- Reimplementing terminal emulation or rendering a full terminal through control mode.
- Preserving every current pixel, binding, or preview behavior at the expense of native integration.
- Transparently moving live Sidebar-owned processes into tmux.
- Preserving processes across reboot; recreating a workspace is not resuming its processes.
- Maintaining two production backends indefinitely.
- Shipping a new remote transport, plugin marketplace, or autonomous agent orchestrator.

### Grounding in the current code

| Area | Current implementation | Intended disposition |
| --- | --- | --- |
| Shell PTYs | `src/pty.rs` | Retire from production after cutover |
| Server, IPC, runtime persistence | `src/server.rs` | Replace management operations with tmux; retain legacy support temporarily |
| Terminal parser and renderer | `src/terminal.rs` | Retire from production terminal display |
| Backend execution and full-screen composition | `src/main.rs` | Separate CLI, legacy runtime, and tmux sidebar runtime incrementally |
| Actions and UI state | `src/state.rs`, `src/input_handler.rs` | Reuse UI concepts; introduce ID-based tmux targets |
| Sidebar and hints | `src/sidebar.rs`, `src/hint_bar.rs` | Reuse within a pane-sized rendering surface |
| Environment and agent launch | `src/env_capture.rs` and launch paths | Adapt explicitly to tmux server environment semantics |
| Tests | Unit tests and `tests/e2e/` | Keep legacy tests until retirement; add isolated tmux integration coverage |

Do not start by extracting a universal backend abstraction from the entire application. Introduce a narrow tmux adapter and reuse UI code as each working slice needs it.

## 2. Invariants and compatibility contract

1. **tmux is authoritative.** Sidebar must observe changes made through other tmux clients, commands, and plugins.
2. **Identity is not a name.** Use session `$id`, window `@id`, and pane `%id`, scoped to a server connection/generation. IDs are not durable across server restart.
3. **Model membership separately.** One window can belong to multiple sessions; an operation on a membership is not necessarily an operation on the underlying window.
4. **Selection has scope.** tmux clients select sessions; sessions have current windows; windows have active panes. Do not promise independent per-client pane focus inside shared windows.
5. **No hidden mutations.** Browsing an existing server initially changes nothing. Enrolling a window in sidebar mode is an explicit layout mutation.
6. **No silent process destruction.** Closing Sidebar is not killing a session or the tmux server. Destructive operations resolve targets again and confirm their actual scope.
7. **No silent configuration takeover.** Never overwrite `~/.tmux.conf`. Bindings/hooks are opt-in, collision-checked, namespaced where possible, and reversible without clobbering later user changes.
8. **No production PTY replacement in tmux mode.** Working applications remain directly in native tmux panes. PTYs used by the test harness are unrelated to this restriction.
9. **Recovery fails safely.** On stale IDs or server restart, discard stale state and rediscover. Do not automatically replay a destructive command after reconnecting.
10. **Legacy work remains accessible.** Migration never stops the existing Sidebar server or deletes its data without explicit authorization.

### Temporary command routing

Step 1 finalized the explicit `sb tmux` route and socket selectors below. Doctor, read-only lists, and the standalone chooser are available. The chooser mutates tmux only for explicit target selection and confirmed empty-server creation. Separate Step 3 demo commands opt in to native sidebar panes on a private disposable server; the architecture is not yet accepted, and general enrollment/management remain future steps. Keep this route stable through the preview period.

- Existing `sb` and existing management commands retain legacy behavior through Step 9.
- `sb tmux ...` is the explicit tmux preview surface, including its TUI launcher.
- `sb tmux --socket-name <name> ...` selects an isolated server; an explicit socket-path option is also supported.
- Socket resolution: explicit argument, then current tmux context, then the default tmux socket. Conflicting selectors are errors. Do not silently create a server for a read-only inspection command.
- Step 10 makes ordinary `sb` commands use tmux and preserves an explicit, temporary legacy route. Step 11 removes the legacy implementation.

## 3. Incremental delivery steps

The steps are sequential delivery gates, not separate working branches. Implement on `main` in small batches with the preview route protecting the default app.

### Step 1 — Read-only tmux inspector

**Usable result:** The original app still works; users can inspect real tmux sessions/windows/panes through `sb`.

**Work**
- Add `sb tmux doctor` and read-only list commands with human-readable and JSON output.
- Introduce typed IDs, server identity, session/window membership, pane records, and a minimal adapter.
- Use argument-safe process execution and explicit tmux output formats, not shell interpolation or human-table scraping. Test delimiters, escaping, and unusual names.
- Detect tmux presence/version, socket errors, empty servers, and unsupported capabilities. Start with local tmux 3.6a; do not claim a minimum supported version until tested.
- Add a tmux test fixture using a unique socket, private configuration, controlled environment, and guaranteed cleanup of only its own server.

**Acceptance demo:** Create two sessions with duplicate window names using tmux. Listing shows distinct IDs and correct membership without creating panes or changing focus. Missing tmux produces an actionable error while legacy `sb` still launches.

**Automated gate:** Parsing/targeting unit tests; isolated read-only integration tests; existing unit suite passes. Compare inventory before/after inspection to prove no mutations.

**Safe stopping point:** A useful diagnostic CLI with no migration required.

### Step 2 — Interactive chooser without layout ownership

**Usable result:** `sb tmux` opens a functional session/window/pane chooser and attaches or switches to the selected target.

**Work**
- Reuse sidebar list, hints, search/navigation, and focus indicators in a standalone manager screen.
- Use bounded snapshot polling initially, plus manual refresh. Keep browsing highlight separate from the committed target.
- Outside tmux, restore the terminal before attaching a native tmux client. Inside tmux, switch the intended existing client instead of nesting tmux.
- Handle zero sessions with an explicit create action using an explicit working directory.
- Reject ambiguous client selection; do not switch an arbitrary attached client.

**Acceptance demo:** Navigate duplicate names, select a pane, and enter the real shell. Detach with native tmux and reopen the chooser. Rename/delete a target externally while browsing; the chooser refreshes without acting on the wrong object.

**Automated gate:** PTY-driven chooser tests; attach versus switch-client routing; empty-server creation; deleted-target errors; terminal restoration on exit/failure.

**Safe stopping point:** A useful tmux launcher/chooser, even if a permanent sidebar proves unsuitable.

### Step 3 — Native sidebar vertical slice and architecture gate

**Usable result:** On a disposable, explicitly managed session, a sidebar stays usable while switching between two real tmux windows. Working terminals use native rendering.

**Work**
- Add an explicit sidebar-mode launcher for managed test sessions. Start with one working pane per window.
- Prototype one marked Sidebar pane per enrolled window; identify it with tmux user options, not its title or process name alone.
- Supply an opt-in binding to enter the sidebar and a reliable action to return to the remembered working pane.
- Prove switching to another window and focusing its sidebar can complete without forwarding navigation keys to a shell.
- Reuse the sidebar rendering at its actual pane dimensions; do not instantiate the legacy terminal renderer.
- Make enrollment idempotent, including simultaneous launch attempts; reopening must not create duplicate sidebar panes.
- Implement show/close and crash/reopen behavior without killing working panes. Respect native zoom; sidebar re-entry must have a defined unzoom behavior.

**Acceptance demo:** Run an editor in one window and a continuously printing process in another. Enter the sidebar, switch windows repeatedly, return to each working pane, resize, zoom/unzoom, close Sidebar, and reopen. Both workloads continue unchanged. Repeat with two clients attached and document the shared-focus/layout behavior.

**Automated gate:** Pane inventory and PID checks before/after sidebar restart; idempotent/concurrent enrollment; resize/focus routing; no keystroke leakage; scoped cleanup.

**Decision gate:** Record the chosen layout/focus strategy and observed multi-client limitations before Step 4. Demonstrate native copy mode and mouse interaction manually in a real terminal. If this strategy cannot meet the UX requirements, retain the Step 2 chooser and seek approval for a popup/chooser-first design. Do not quietly switch to a custom terminal renderer.

**Safe stopping point:** A working, explicitly experimental sidebar on managed sessions, plus the reliable chooser fallback.

### Step 4 — Daily-use window and session management

**Usable result:** A user can conduct an ordinary single-pane working day entirely through the native sidebar.

**Work**
- Implement create, rename, select, move, and close-window operations; create, rename, switch, and close-session operations.
- Preserve confirmations, contextual hints, last-window navigation, and an explicit detach path.
- Map actions to typed targets rather than globally unique names. Resolve fresh membership before mutations.
- Handle linked windows explicitly: distinguish unlinking from destroying the underlying window; show all affected sessions before a destructive action.
- Define empty-state behavior when the final working pane/window/session disappears. Do not accidentally leave an invisible sidebar-only workspace.
- Specify whether window reordering changes tmux indexes or only Sidebar display order; expose the distinction rather than implying equivalence.
- Create windows with explicit cwd/environment policy. Sidebar focus after each action must be intentional.

**Acceptance demo:** Create two projects, rename a duplicate-named window, move it, switch projects, cancel a deletion, then confirm a deletion. Close the final working window and get a clear usable empty state or return to the launcher.

**Automated gate:** Lifecycle operations; duplicate/special names; linked-window scope; canceled deletion is a no-op; last-window/session behavior; move target disappearing mid-action.

**Safe stopping point:** A functional basic tmux manager; pane editing and advanced integration can wait.

### Step 5 — Live synchronization and recoverable connections

**Usable result:** Sidebar remains accurate when tmux is changed elsewhere and survives connection disruptions safely.

**Work**
- Add a notification-driven connection, preferably tmux control mode used for management events, not terminal rendering. Retain bounded full-snapshot reconciliation for missed events/reconnects.
- Use a single owner/parser for each protocol stream. Route command responses and unsolicited notifications explicitly; do not mix synchronous reads with asynchronous draining.
- Suppress unused pane-output subscriptions where supported and avoid unnecessary history capture. Ensure observer clients do not unexpectedly influence working-pane sizing or session lifecycle.
- Coalesce bursts, expose disconnected/stale state, and recover after Sidebar restart or tmux socket replacement.
- Tie cached IDs to a server generation. Disable mutations until reconnect and fresh discovery complete.
- Make highlighting and UI state resilient to reordered/deleted objects and membership changes.

**Acceptance demo:** While Sidebar is open, use another tmux client to create, rename, split, move, and delete objects. Sidebar converges without manual refresh. Restart a disposable tmux server; Sidebar reconnects to the new inventory without replaying old actions.

**Automated gate:** Interleaved response/notification fixtures; event storms; server replacement with reused IDs; bounded reconciliation latency (target: within two seconds on a local idle server); no observer-induced resize.

**Safe stopping point:** A manager suitable for use alongside native commands and other tmux tooling.

### Step 6 — Safe integration with existing tmux setups

**Usable result:** Users can adopt Sidebar in an existing server without surrendering their configuration or ordinary tmux workflows.

**Work**
- Extend enrollment to existing windows only on explicit request. Show the proposed pane/layout change and preserve working panes.
- Manage Sidebar markers and integration resources with clear ownership. Native-created windows appear in the chooser immediately but are not automatically modified without an explicit session enrollment policy.
- Add integration enable/disable/doctor actions, conflict detection, and reversible binding/hook installation. Do not silently replace the user's prefix with `Ctrl+B` behavior.
- Handle external sidebar-pane removal, newly linked windows, native layout changes, zoom, small terminals, and duplicate Sidebar launch.
- Provide graceful degradation to the chooser when pane insertion is impossible or unwanted.
- Define tested behavior for two clients sharing a session/window. Do not advertise client-private sidebars: pane layout and active-pane state can be shared.
- On disable, remove only owned resources. Restore a saved layout only if still applicable; do not overwrite legitimate user splits made since enrollment.

**Acceptance demo:** Enable Sidebar on a populated, custom-configured tmux server, use native commands and bindings, open a second client, then disable Sidebar. Working processes survive and unrelated configuration remains unchanged.

**Automated gate:** Existing custom bindings/hooks; repeat enable/disable; external pane deletion; linked-window enrollment; two-client behavior; narrow terminals; no destructive cleanup of user resources.

**Safe stopping point:** A usable tmux extension for existing users, not just Sidebar-created sessions.

### Step 7 — First-class pane and layout management

**Usable result:** Sidebar is now a session → window → pane manager, rather than a window picker with one shell per window.

**Work**
- Add an expandable pane tree, stable selection, pane labels, active indicators, and search across sessions/windows/panes.
- Support split horizontally/vertically, select, resize, zoom, close, and a bounded set of layout actions.
- Exclude marked sidebar panes from ordinary workload actions and destructive pane counts.
- Apply layouts with an explicit sidebar policy: preserve/rebuild the sidebar region or show the resulting limitation. Native layout presets include all panes unless deliberately adapted.
- Preserve native copy mode, mouse handling, and terminal input; do not rebuild them in Rust.

**Acceptance demo:** Build a workspace with an editor, server, and logs; find each through the tree; resize/zoom; change layout; close a working pane without touching Sidebar or other workloads.

**Automated gate:** Pane lifecycle/targeting; filtering sidebar panes; layout geometry at supported sizes; linked windows; externally split panes; native copy-mode smoke checks.

**Safe stopping point:** A powerful general-purpose tmux manager. More elaborate workflow automation remains optional.

### Step 8 — Resolve Sidebar-specific UX and preference parity

**Usable result:** The tmux app has a deliberate, documented replacement for each important legacy interaction.

**Work**
- Finalize browse-versus-commit semantics, cancel/return behavior, last-window navigation, contextual hints, help, and session dialogs.
- For native sidebar mode, prefer explicit selection/commit with no cross-window preview initially. Add snapshot previews only if useful; label snapshots and do not treat `capture-pane` as a live terminal protocol.
- Document that switching native windows to preview them would mutate tmux selection and may affect other clients. Do not silently preserve legacy preview semantics by doing this.
- Persist UI preferences separately from tmux inventory: tree expansion, display order, sidebar width, and relevant selection hints. Never revive a stale tmux ID from disk.
- Keep scrollback/copy state under tmux ownership. Use a sidebar-local dialog or supported popup instead of drawing over working terminal cells directly.
- Publish a feature parity matrix with each legacy feature marked retained, intentionally changed, deferred, or removed. No unexplained disappearance of mouse, zoom, preview, restoration, or agent behavior.

**Acceptance demo:** Perform the documented keyboard workflow without a mouse, including browse/cancel, return to work, help, and reopen. Verify unsupported bindings never reach a working shell from sidebar focus.

**Automated gate:** Navigation and confirmation regressions; preferences across restart; missing targets; supported compact sizes; full matrix linked to tests or explicit manual checks.

**Safe stopping point:** A coherent replacement experience rather than an assortment of tmux commands.

### Step 9 — Project/agent launch and non-destructive legacy import

**Usable result:** Users can move their project organization to tmux and launch repeatable working environments while finishing old work in legacy Sidebar.

**Work**
- Restore the existing agent-launch workflow using native tmux panes, explicit cwd, and an allowlisted environment policy. Window shells inherit from the tmux server unless environment is deliberately supplied or updated.
- Add simple saved project launch definitions for directories, windows, commands, and optional layouts. Require explicit execution of configured commands; never execute commands merely by importing metadata.
- Implement a read-only import preview of legacy sessions/window names/cwds, then an explicit, idempotent apply operation with conflict reporting.
- Record import provenance separately from runtime tmux IDs so repeats do not duplicate work. Handle name conflicts, invalid cwd, and partial failure without deleting existing objects.
- Preserve legacy files/server untouched. If old scrollback is offered, expose it as an archive, not fabricated live terminal/process state.
- Clearly distinguish restartable workspace definitions from process survival across reboot.

**Acceptance demo:** Leave a long-running job in legacy Sidebar. Preview and apply an import, open the resulting tmux shells, launch an agent, and repeat the import without duplicates. The legacy job is still running and accessible.

**Automated gate:** Legacy metadata fixtures; import dry-run is read-only; repeated/partial imports; conflict resolution; missing cwd/agent executable; environment secrets not persisted into project definitions; legacy PID survives.

**Safe stopping point:** Users can voluntarily migrate without a forced cutover.

### Step 10 — Make tmux the default, retain an explicit escape hatch

**Usable result:** Installing and running `sb` starts the supported tmux-backed product. Legacy access is explicit and temporary.

**Work**
- Map ordinary `sb` UI/CLI commands onto tmux. Preserve aliases where meanings remain safe; ambiguous old name-based targeting must produce a disambiguation error.
- Specify legacy-only command behavior (`server`, `stale`, `restore`, `forget`, `shutdown`) explicitly. Never reinterpret legacy `shutdown` as an unconfirmed tmux `kill-server`.
- Keep an explicit legacy launcher/command route for one documented deprecation release cycle. Both modes must identify themselves clearly.
- Finalize minimum tmux version from tested macOS/Linux builds and add capability checks. Native Windows is no longer supported by the tmux backend; document WSL as the supported Windows route and preserve access to the last compatible legacy release.
- Update Homebrew/AUR dependencies, npm/curl prerequisite guidance, release artifacts, self-update messaging, and installation/upgrade docs. Do not install tmux silently through an arbitrary package manager.
- Provide setup diagnostics and explicit consent before starting servers or modifying tmux configuration.

**Acceptance demo:** On clean macOS and Linux environments, install and launch into a working tmux session; repeat inside an existing tmux client. On a machine without tmux, show actionable setup guidance rather than a panic. Upgrade a legacy installation and demonstrate both import and legacy access.

**Automated gate:** Installer/package checks; routing and alias tests; missing/old tmux; legacy-only command safety; clean install and upgrade smoke tests. A release review runs the full applicable E2E suite, not only new feature tests.

**Safe stopping point:** A shipped tmux-first app with a documented rollback path. Never downgrade by killing either server.

### Step 11 — Retire the custom multiplexer

**Usable result:** A smaller tmux-only application with no production shell PTY/server/terminal-emulation stack.

**Prerequisite:** Step 10 has completed its deprecation cycle, the parity matrix is accepted, and no release-blocking tmux regressions remain. This step is intentionally not part of the initial preview release.

**Work**
- Remove legacy server startup, IPC, shell PTY ownership, and full terminal rendering from production paths.
- Remove unused `portable-pty`/`vt100` dependencies only if no remaining production need exists; test-only terminal harnesses may retain appropriate dependencies.
- Keep a narrowly scoped legacy metadata reader if import remains supported; it must not boot or manage the old server.
- Preserve legacy data on disk unless the user explicitly requests cleanup. Document running a pinned legacy binary against old workloads and keep an accessible compatible release.
- Retire tests of removed internals while retaining equivalent user-workflow coverage and import fixtures.
- Verify closing/crashing Sidebar never terminates native workloads and that no hidden Sidebar-owned shell server starts.

**Acceptance demo:** Launch, manage sessions/windows/panes, run an agent, detach, terminate Sidebar, and reconnect using plain tmux. Workloads remain. Import old metadata with the legacy implementation absent.

**Automated gate:** Dependency/production-path audit; full applicable E2E suite during final review; process-ownership assertions; import tests; supported-platform release smoke checks.

**Safe stopping point:** Migration complete. Subsequent work is product development on top of tmux.

## 4. Definition of done for every implementation step

A step is complete only when all of the following hold:

- Its acceptance demo works in the installed CLI, not just in unit-test mocks.
- All previously delivered tmux functionality remains usable; legacy behavior remains intact until its declared cutover/removal.
- New functionality has unit/integration coverage, and relevant existing E2E tests pass.
- Existing behavior changes include a nearby simple code comment explaining the old limitation and the new approach, per project conventions.
- Errors, cancellation, stale targets, and restart behavior are exercised, not just the happy path.
- User-facing command/help documentation identifies preview limitations and any changed behavior.
- Build and reinstall locally after each functioning implementation batch: `cargo install --path . --force`. Avoid `install.sh` for routine batches because it also increments the package version and may deploy to a container.
- Record exact validation commands/results and unresolved limitations in the implementation handoff. Do not check off a later stage based on an earlier mock or prototype.

### Test execution policy

- Run `cargo test --lib` after code changes.
- Add focused tmux integration and PTY-driven E2E tests to the existing harness or a clearly separated tmux test target. tmux-specific CI jobs must install the supported dependency; do not silently skip the entire feature suite when tmux is missing.
- For E2E tool calls, follow `AGENTS.md`: Bash timeouts are **seconds**. Use `timeout: 90` around the runner's default 60-second whole-run budget; explicit full reviews may use `--run-timeout 180` and Bash `timeout: 210`. The former `600000` requirement confused milliseconds with seconds and is obsolete. Capture **both** streams with `2>&1 | tee` and pipeline failure propagation so a passing `tee` cannot hide failed tests.
- During development, run feature-related E2E tests. Full E2E runs belong to review/release gates, including Steps 10 and 11; do not run the slow full suite for every small edit.
- Every fixture owns an isolated tmux socket and config. Cleanup must never target the user's default server. Set test environment variables on the initial server-starting command, not merely on a later client.
- Automate structural assertions (IDs, pane geometry, process survival, focus targets) alongside TUI interaction tests. Use manual real-terminal checks for native mouse, copy mode, visual behavior, and representative user configurations.
- Documentation changes: run `npm run docs:build`.

## 5. Delivery groups and decision checkpoints

| Group | Steps | What can ship |
| --- | --- | --- |
| Read-only/chooser preview | 1–2 | Useful tmux inspection and navigation; no legacy replacement |
| Architecture proof | 3 | Experimental native sidebar; explicit go/no-go on its UX |
| Core daily-use preview | 4–6 | CRUD, live state, safe existing-server integration |
| Feature-complete candidate | 7–9 | Panes, UX parity decisions, project/agent workflows, opt-in import |
| Default release | 10 | tmux-first product with temporary legacy access |
| Retirement release | 11 | Custom multiplexer removed after the deprecation interval |

Do not estimate the whole migration as a fixed short rewrite before Step 3. Re-estimate after the native sidebar proof and again after existing-server/multi-client integration in Step 6. If schedule pressure appears, defer optional previews and launch-template sophistication, not targeting safety, process survival, or synchronization.

## 6. Risks to resolve explicitly

| Risk | Resolution/gate |
| --- | --- |
| Sidebar panes disrupt layouts or flicker during switching | Step 3 proves the mechanism; Step 6 handles adoption; Step 7 validates layout actions |
| Shared clients fight over focus or width | Step 3 documents native semantics; Step 6 tests and exposes limitations |
| Existing config or plugins conflict with integration | Opt-in, reversible installation and chooser fallback in Step 6 |
| Duplicate names, linked windows, recycled IDs cause wrong-target mutations | Typed server-scoped identity from Step 1; destructive scope in Step 4; generation reset in Step 5 |
| Notifications race command responses or observer clients affect sizing | Dedicated protocol ownership and observer tests in Step 5 |
| Preview parity recreates a terminal emulator | Explicit native interaction contract in Step 8; no hidden terminal-renderer fallback |
| Users expect live migration or reboot process restoration | Dry-run metadata import and explicit persistence limits in Step 9 |
| Existing Windows users receive a nonfunctional automatic update | Platform/release-artifact and upgrade policy required before Step 10 |

## 7. Completion checklist

- [x] Step 1: Read-only inspector
- [x] Step 2: Interactive chooser
- [ ] Step 3: Native sidebar proof and architecture decision accepted
- [ ] Step 4: Window/session lifecycle
- [ ] Step 5: Live synchronization and reconnect safety
- [ ] Step 6: Existing-server integration and multiple-client contract
- [ ] Step 7: Pane/layout management
- [ ] Step 8: UX/preference parity matrix accepted
- [ ] Step 9: Project/agent launch and legacy import
- [ ] Step 10: Default cutover and deprecation release
- [ ] Step 11: Legacy implementation retired

## Related material

- [Current terminology and compatibility](./terminology.md) — what the current release actually implements.
- [Current keybindings](./keybindings.md) — baseline for the eventual parity matrix.
- [Current sessions guide](./sessions.md) — existing organization behavior.
- [Earlier tmux hotkey comparison](./tmux_hotkey_comparison.md) — prior proposals, not an implemented backend contract.
