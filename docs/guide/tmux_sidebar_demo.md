# Native sidebar architecture demo (Step 3, not yet accepted)

**Incomplete checkpoint:** automated checks pass, but real-terminal mouse/copy-mode evidence and explicit architecture acceptance are still required. Do not proceed to Step 4 based on this prototype alone. The [standalone chooser](./tmux_preview) remains available unchanged.

## Restricted scope

This experiment only creates or reopens its own disposable server, using an **explicit `--socket-path` in an owned directory with mode 0700**. It refuses ordinary existing servers. It creates one marked session with two marked windows (`editor`, `logs`), one working pane and one Sidebar pane in each. It never edits `~/.tmux.conf`; a fresh demo server starts with `/dev/null` as its config.

Sidebar panes are identified by tmux user options, not titles or process names. Working-pane IDs are retained on their windows and validated before every operation. The stored Sidebar process PID must also match: an externally respawned application is not killed merely because it inherited an old role marker. Native-created windows are not implicitly enrolled. Extra working splits, linked/multiple sessions, or missing original working panes are outside this proof's contract: retain the chooser rather than guessing repairs.

| Command | Effect |
| --- | --- |
| `sb tmux --socket-path PATH sidebar-demo --cwd ABSOLUTE_DIR` | Create/reopen the dedicated two-window proof, focus first sidebar; print attachment instructions |
| Add `--enable-binding` to `sidebar-demo` | Opt in to root-table F12 sidebar focus; reject an existing or subsequently changed binding |
| `sb tmux --socket-path PATH sidebar-show [--window '@ID']` | Reopen missing marked sidebars and focus one; default is first enrolled window |
| `sb tmux --socket-path PATH sidebar-close` | Close only marked Sidebar panes; preserve working panes and their processes |
| Add `--disable-binding` to `sidebar-close` | Remove F12 only if it still matches the exact installed binding; refuse to overwrite later edits |

These commands do not accept JSON or inferred socket selection. The normal `--socket-name` inspector/chooser route is unchanged; this restricted prototype requires a private socket **path**.

## Layout and focus strategy under evaluation

- Native left-hand split, initially 28 columns, per marked window. The sidebar renders metadata at its actual pane size. Working applications render directly in sibling tmux panes; no Sidebar terminal parser/renderer is instantiated.
- `j`/`k` browse without native selection. **Enter goes to the highlighted window's sidebar**, not its shell. Destination sidebar focus is selected *before* the window becomes visible.
- **Tab or Esc returns to the original working pane of the current window.** In this slice there is only one working pane per window; general remembered-pane selection waits for pane management.
- **F12**, when explicitly enabled, uses native conditional bindings to select a literal marked sidebar ID synchronously. It does not start a background shell launcher that could leak following navigation keys.
- Entry through F12/show unzooms natively (`select-pane` without `-Z`). No automatic re-zoom on returning to work.
- **q or Ctrl+C closes only the currently focused Sidebar process/pane.** After closing/crashing a sidebar, reopen with `sidebar-show` from another terminal. F12 is not a process supervisor; stale pane targets report a native error until reopening refreshes the owned binding.
- A kernel file lock serializes launch/show/close operations and metadata readers during split enrollment. Repeated/concurrent launches do not create duplicate panes. The private `.sidebar_demo.lock` file is deliberately retained to avoid unlink/relock races; a crashed owner releases its kernel lock.

**Multi-client limitation:** layout, pane widths, window focus, and pane focus are native shared state. Two clients attached to the same session see the same selected window/pane. Resizing either client may resize the native panes; 28 columns is an initial size, not an enforced preference. This is not a client-private sidebar. No claim is made yet about general adoption into existing configurations.

## Required real-terminal acceptance — please report evidence

Run in an ordinary terminal outside tmux, using the installed CLI. Do not substitute your usual tmux socket.

```bash
export SB_BIN="$HOME/.cargo/bin/sb"
export SB_DEMO_DIR="$(mktemp -d /tmp/sb-native-demo.XXXXXX)"
export SB_DEMO_SOCKET="$SB_DEMO_DIR/socket"
chmod 700 "$SB_DEMO_DIR"
"$SB_BIN" --version
tmux -V
SHELL=/bin/sh "$SB_BIN" tmux --socket-path "$SB_DEMO_SOCKET" \
  sidebar-demo --cwd "$SB_DEMO_DIR" --enable-binding

# Explicit native settings for this private manual mouse/copy-mode test only:
tmux -S "$SB_DEMO_SOCKET" set-option -g mouse on
tmux -S "$SB_DEMO_SOCKET" set-window-option -g mode-keys vi

tmux -S "$SB_DEMO_SOCKET" list-panes -a \
  -F '#{window_id} #{pane_id} #{pane_pid} #{@sb_demo_panel}'
printf 'Demo socket for the second terminal: %s\n' "$SB_DEMO_SOCKET"
tmux -S "$SB_DEMO_SOCKET" attach-session -t sidebar-demo
```

1. In the `editor` sidebar, press **Tab** and run `vi "$SB_DEMO_DIR/editor.txt"`. Enter some text and save it. Leave the editor running.
2. Press **F12**, **j**, **Enter**, **Tab**. In the `logs` working pane, run `while :; do date; sleep 1; done`.
3. Use **F12**, **k/j**, **Enter** repeatedly to switch sidebars. Use **Tab** to return to each workload. Confirm no navigation keys appear in the editor or reach the logging shell.
4. While a working pane is focused, use native prefix **Ctrl+B**, then **z** to zoom. Press **F12** and confirm deterministic unzoom/sidebar focus. Resize the terminal and drag the native pane border with the mouse. Check the sidebar remains usable and both workloads continue.
5. In the logs pane, press native prefix **Ctrl+B**, then **[**. Navigate history, press **Space** to start a selection (vi copy mode), move, and press **Enter** to copy. Report whether native copy/scrollback worked. Also test mouse wheel/history and selecting/copying text with the mouse. Report actual results, not just that tmux accepted a command.
6. Open a second terminal, assign `SB_DEMO_SOCKET` to the **exact private path printed above**, and run `tmux -S "$SB_DEMO_SOCKET" attach-session -t sidebar-demo`. Switch/focus and resize from both clients. Record whether the shared selection/layout is acceptable.
7. Press **q** in one sidebar. Verify the working editor/logging process survives. From the second terminal after detaching its native client (Ctrl+B, d), run `"$HOME/.cargo/bin/sb" tmux --socket-path "$SB_DEMO_SOCKET" sidebar-show`. Reopen without duplicates; compare `list-panes` working IDs/PIDs with the initial output. Automated tests separately cover killing a Sidebar display process and reopening it.
8. From the detached second terminal, run `"$HOME/.cargo/bin/sb" tmux --socket-path "$SB_DEMO_SOCKET" sidebar-close --disable-binding`. Both working panes should remain. Reattach with plain tmux and verify the editor and logs are still live.

Send back:

- Terminal application/OS and tmux version; installed `sb` path.
- Mouse and native copy-mode results, with screenshots/recording or a precise observed transcript.
- Focus/resize/zoom, two-client behavior, no-keystroke-leak, and workload survival results (include pane/PID output).
- Explicit **accept** or **reject** for the pane-per-window architecture and its shared-focus/width limitations.

If rejected, retain the Step 2 chooser and discuss a chooser/popup-first alternative. Do not replace native rendering with a custom terminal renderer. Re-estimate remaining work after the architecture decision is accepted; that decision is still pending.

### Optional cleanup, only after finishing

These commands **stop the demo editor/logs**, so run them only when you intend to discard that test work. Verify the path refers to the private demo you just created, never your normal socket:

```bash
printf '%s\n' "$SB_DEMO_SOCKET"
# Only after verifying this is your disposable test server:
tmux -S "$SB_DEMO_SOCKET" kill-server
```

No cleanup of legacy Sidebar data is needed or performed. Keep the private directory if you want the editor file or evidence; remove it manually only after reviewing its contents.

## Automated evidence (not the manual gate)

```bash
set -o pipefail
SB_TMUX_TEST_BINARY="$HOME/.cargo/bin/sb" cargo test --test tmux_sidebar \
  2>&1 | tee /tmp/sb_step3_installed_sidebar.txt
```

Tests assert concurrent/idempotent enrollment, display-process crash/reopen, unchanged working IDs/PIDs, zero raw navigation bytes arriving at working `cat` processes, native zoom/focus/resize routing, shared two-client selection, copy-mode state, opt-in binding collisions (including repeat bindings), later-edit preservation, and rejection of unowned servers/inferred socket authorization. The terminal parser used in these tests is test-only.
