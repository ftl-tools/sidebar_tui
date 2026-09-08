# Sessions

Sessions contain windows, using tmux terminology. Organize sessions by project or context. Each window currently has one terminal pane; split panes are not implemented.

## Session chooser

Focus the sidebar with <kbd>Ctrl+B</kbd> or <kbd>Ctrl+Space</kbd>, then press <kbd>s</kbd>. Navigate with arrows or <kbd>j</kbd>/<kbd>k</kbd> and press <kbd>Enter</kbd> to switch. The active session has a `*`. <kbd>Esc</kbd> or <kbd>q</kbd> closes the chooser.

- **Create:** press <kbd>C</kbd>, type a name, then <kbd>Enter</kbd>.
- **Rename:** select a session, press <kbd>R</kbd> or <kbd>$</kbd>, edit its name, then <kbd>Enter</kbd>.
- **Kill:** select a session, press <kbd>K</kbd>, then confirm with <kbd>y</kbd>. This terminates all its windows.

The same uppercase commands work from the sidebar on the current session. <kbd>P</kbd>/<kbd>N</kbd> or direct <kbd>Alt+Up</kbd>/<kbd>Alt+Down</kbd> switch sessions.

## Windows and saved state

- Every window belongs to exactly one session. New windows are created in the active session.
- Windows keep running when you switch sessions or detach the client.
- Each session remembers its selected window, focus region, and sidebar scroll position. Window scroll positions are restored when switching back.
- Session metadata persists across restarts. Killing the last session automatically creates a new **Default** session.
- To move a window, highlight it in the sidebar, press <kbd>m</kbd>, choose a destination session, and press <kbd>Enter</kbd>.

## CLI

```sh
sb session list
sb session create project
sb session switch project
sb list-windows
sb kill-window api-server
sb session kill project
```

Window names currently remain globally unique, unlike tmux's per-session window indexes. See [terminology and compatibility](./terminology.md) for backend limitations and legacy aliases.
