# tmux chooser and inspection preview

Ordinary `sb` still uses Sidebar's legacy PTY server. Bare `sb tmux` opens a standalone native-target chooser. The explicit doctor/list subcommands remain strictly read-only. Neither mode installs hooks, inserts sidebar panes, owns layouts, or edits your configuration.

A separate [native sidebar architecture demo](./tmux_sidebar_demo) is available only on explicitly created disposable servers. Step 3 remains incomplete pending real-terminal evidence and architecture approval; it is not general enrollment support.

## Commands

```bash
sb tmux                       # Interactive chooser; terminal required
sb tmux --socket-name my_server
sb tmux --cwd /absolute/project/path  # Enables confirmed creation when empty
sb tmux doctor
sb tmux list-sessions
sb tmux list-windows --json
sb tmux list-panes --json
sb tmux --socket-name my_server doctor
sb tmux --socket-path /absolute/path/to/socket list-windows
```

Every inspection command supports `--json`; the chooser rejects it. Human output shows record counts and labeled fields; names and titles are quoted so embedded control characters cannot affect your terminal. JSON listings include the server identity alongside the requested records. Windows have separate `memberships` describing their session, index, and active status. Linked windows/panes are not duplicated in the underlying object lists.

Socket selection is: explicit name/path, then the socket in `$TMUX`, then tmux's default socket (including tmux's normal `TMUX_TMPDIR` behavior). Name and path together are an error. Explicit selectors override inherited context. Malformed context requires an explicit selector rather than silently inspecting another server.

## Chooser keyboard workflow

The list displays **session / window / pane IDs and names** per membership, with native active-window/active-pane indicators (`W*`/`P*` active, `W-`/`P-` inactive). IDs and indicators precede names so long names cannot hide target identity. Its purple outline and `>` highlight indicate chooser focus, not a committed native selection. Linked windows appear under each session membership. This is a flat searchable hierarchy, not the expandable pane manager planned for Step 7.

| Key | Action |
| --- | --- |
| Arrows, `j`/`k`, `n`/`p` | Browse; never switch native windows to preview them |
| `/` | Search all displayed names, titles, and IDs |
| Enter while searching | Finish search without attaching |
| Esc while searching | Clear search and return to browsing |
| Enter while browsing | Resolve the highlighted membership/pane again and commit |
| `r` | Refresh immediately |
| `q`, Esc, Ctrl+C | Close chooser; do not kill workloads |
| `c` when no sessions exist | Request creation (requires absolute existing `--cwd`) |
| `y` / `n` or Esc in creation prompt | Confirm / cancel creation |

Snapshots refresh 750 ms after the previous poll completes. Highlight survives rename/reordering by typed target, not by name or row number. Deleted targets, disconnected state, and server generation changes clear selection: navigate explicitly before committing again. No action is automatically replayed. Unsupported keys are consumed, not forwarded to a shell.

Outside tmux, the chooser leaves raw/alternate-screen mode **before** selecting and attaching a native tmux client. Detach using your native tmux binding (default prefix then `d`), then reopen `sb tmux`.

Inside tmux, it switches an existing client rather than nesting tmux. Client candidates must be attached to a session containing the invoking `$TMUX_PANE`. More than one candidate requires explicit `--client NAME`, where NAME is the exact client name/TTY from `tmux list-clients -F '#{client_name}'`. This deliberately does not guess which terminal you meant. Invalid clients, missing context, and attempts to switch across servers are rejected. Reopen after the invoking server restarts.

**Shared focus is native tmux behavior:** selecting a window changes session selection; selecting a pane changes window focus and can affect other clients. Selection requests are sequential, not rollback transactions. A native attach failure can leave the explicitly selected target active, but never kills it. Canceling browsing itself is read-only.

For an absent or empty server, creation is opt-in with `--cwd`, `c`, then `y`. It creates a detached session and returns to the chooser; navigate and Enter to attach. tmux uses its normal configuration and server environment. Sidebar writes no configuration and makes no silent package installations. Creation rechecks emptiness; a newly populated server requires refresh instead. Canceling afterward leaves explicitly created workloads intact.

## Diagnostics and limits

- This preview is validated on **local macOS tmux 3.6a only**. Other binary versions are rejected; this is not a minimum-version or Linux support claim. Install tmux yourself and ensure it is on `PATH`.
- Missing/unreachable sockets return a nonzero error. No server is started. If desired, create your own session explicitly with tmux and retry. A running server retained with `exit-empty off` returns empty lists when it has no sessions.
- Missing tmux does not prevent ordinary legacy `sb` from launching.
- Unsupported format capabilities or non-UTF-8 inventory produce errors, not guessed records.
- IDs (`$session`, `@window`, `%pane`) belong to the returned server observation. The socket, server PID/start time, and fresh observation token scope them. Never persist or compare bare IDs across observations/restarts.
- Inspection makes multiple read-only requests, not an atomic transaction. Detected server replacement or inconsistent references fail with a retry error. Concurrent changes can still yield a mixed-time view; rerun inspection. There is no cache or action replay.
- There are no destructive actions. `Ctrl+C` stops inspection normally without owning workloads; the chooser also restores its terminal mode on Ctrl+C, ordinary cancellation, or returned errors. Inspection never changes terminal mode. Forced process termination (for example SIGKILL) cannot guarantee terminal restoration.
- Polls use synchronous local tmux requests; the interval limits polling frequency, not the response time of a stalled server. Notification-driven connection handling is deferred to Step 5.

## Reproduce the isolated acceptance demo

The test fixtures own unique private sockets, configuration, environment, and cleanup. They never target your normal tmux or legacy server. Missing tmux fails rather than skipping validation.

```bash
cargo install --path . --force
set -o pipefail
SB_TMUX_TEST_BINARY="$HOME/.cargo/bin/sb" cargo test \
  --test tmux_chooser --test tmux_inspector --test tmux_legacy_launch \
  2>&1 | tee /tmp/sb_step1_installed.txt
```

Chooser PTY tests additionally exercise browse/search/cancel, external rename/delete/restart, native shell attachment/detachment/reopen, inside-tmux switching with ambiguous-client rejection, explicit creation/cancellation with a literal special-character cwd, and terminal restoration on attach failure.

The inspector tests run the **installed CLI** against two duplicate-named windows in separate sessions, adds a linked membership, and compares native inventory/PIDs/focus/layout before and after all inspections. It also checks unusual UTF-8/control-character names, external deletion, same-socket restart, empty inventory, socket failures, missing/old tmux, and legacy PTY launch/detach without tmux.

See the [migration plan](./tmux_migration_plan) and [validation handoff](./tmux_migration_progress).
