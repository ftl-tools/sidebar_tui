# Terminology and compatibility

Sidebar is moving toward being a tmux manager. Its code, UI, and documentation now use tmux's hierarchy:

| Former Sidebar term | Current term | Meaning |
|---|---|---|
| Workspace | Session | A named collection of windows |
| Session / terminal session | Window | A named entry in the sidebar |
| Terminal view | Terminal pane | The terminal process displayed within a window |
| Sidebar pane | Sidebar / focus region | The window picker, not a tmux pane |
| Daemon | Server | The background process that owns the terminals |
| Quit | Detach | Close the client without killing windows |
| Delete a session or terminal | Kill a session or window | Terminate its processes |

**This release does not use tmux as its backend.** Sidebar still owns its PTYs and terminal emulation. There is currently one terminal pane per window, no pane splitting or linking windows between sessions, and window names remain globally unique. Saved metadata/scrollback restoration is Sidebar functionality, not a claim that tmux preserves processes across reboot.

## CLI compatibility

Preferred commands are `sb list-windows`, `sb kill-window <window>`, `sb session list|create|switch|kill`, and `sb server`. Launch with `sb --window <window>` (or `-w`). `sb attach <window>` opens a Sidebar client on a window, creating that window if needed; it is not tmux's `attach-session` command.

Existing scripts continue to work:

- `sb list` and `sb kill` alias the window commands.
- `sb workspace …` aliases `sb session …`; `delete` aliases session `kill`.
- `sb daemon` aliases `sb server`.
- `--session` and `-s` retain their **old window-targeting meaning** as hidden compatibility aliases. Prefer `--window` to avoid ambiguity.

## Saved data and running servers

No manual data migration or server shutdown is needed for the terminology change. The legacy `sessions/` directory still contains window metadata and state, `workspaces.json` still contains session metadata, and the IPC socket/port names are unchanged. Explicit serde wire names preserve the existing JSON protocol and saved fields, so a new client can connect to an already-running old server.

Compatibility names intentionally remain in serialization attributes and migration tests. Third-party references and archived progress snapshots retain their historical vocabulary. The older guide URL `/guide/workspaces` remains as a compatibility page.
