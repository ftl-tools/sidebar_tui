# Read-only tmux preview

Ordinary `sb` still uses Sidebar's legacy PTY server. The explicit `sb tmux` route only inspects tmux; it does not attach, create servers, change focus/layouts, install hooks, or edit configuration. A chooser comes in a later migration step.

## Commands

```bash
sb tmux doctor
sb tmux list-sessions
sb tmux list-windows --json
sb tmux list-panes --json
sb tmux --socket-name my_server doctor
sb tmux --socket-path /absolute/path/to/socket list-windows
```

Every command supports `--json`. Human output shows record counts and labeled fields; names and titles are quoted so embedded control characters cannot affect your terminal. JSON listings include the server identity alongside the requested records. Windows have separate `memberships` describing their session, index, and active status. Linked windows/panes are not duplicated in the underlying object lists.

Socket selection is: explicit name/path, then the socket in `$TMUX`, then tmux's default socket (including tmux's normal `TMUX_TMPDIR` behavior). Name and path together are an error. Explicit selectors override inherited context. Malformed context requires an explicit selector rather than silently inspecting another server.

## Diagnostics and limits

- This preview is validated on **local macOS tmux 3.6a only**. Other binary versions are rejected; this is not a minimum-version or Linux support claim. Install tmux yourself and ensure it is on `PATH`.
- Missing/unreachable sockets return a nonzero error. No server is started. If desired, create your own session explicitly with tmux and retry. A running server retained with `exit-empty off` returns empty lists when it has no sessions.
- Missing tmux does not prevent ordinary legacy `sb` from launching.
- Unsupported format capabilities or non-UTF-8 inventory produce errors, not guessed records.
- IDs (`$session`, `@window`, `%pane`) belong to the returned server observation. The socket, server PID/start time, and fresh observation token scope them. Never persist or compare bare IDs across observations/restarts.
- Inspection makes multiple read-only requests, not an atomic transaction. Detected server replacement or inconsistent references fail with a retry error. Concurrent changes can still yield a mixed-time view; rerun inspection. There is no cache or action replay.
- There are no destructive actions or confirmation dialogs to cancel. `Ctrl+C` stops the inspector normally without owning any workloads. No terminal mode is changed by inspection.

## Reproduce the isolated acceptance demo

The test fixtures own unique private sockets, configuration, environment, and cleanup. They never target your normal tmux or legacy server. Missing tmux fails rather than skipping validation.

```bash
cargo install --path . --force
set -o pipefail
SB_TMUX_TEST_BINARY="$HOME/.cargo/bin/sb" cargo test \
  --test tmux_inspector --test tmux_legacy_launch \
  2>&1 | tee /tmp/sb_step1_installed.txt
```

This runs the **installed CLI** against two duplicate-named windows in separate sessions, adds a linked membership, and compares native inventory/PIDs/focus/layout before and after all inspections. It also checks unusual UTF-8/control-character names, external deletion, same-socket restart, empty inventory, socket failures, missing/old tmux, and legacy PTY launch/detach without tmux.

See the [migration plan](./tmux_migration_plan) and [validation handoff](./tmux_migration_progress).
