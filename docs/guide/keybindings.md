# Keybindings

All modifier keys use <kbd>Ctrl</kbd> on Mac, Windows, and Linux.

## Terminal pane

| Key | Action |
|-----|--------|
| <kbd>Ctrl+B</kbd> | Focus sidebar |
| <kbd>Ctrl+N</kbd> | New session (create mode) |
| <kbd>Ctrl+Z</kbd> | Toggle zoom (hide/show sidebar) |
| <kbd>Ctrl+W</kbd> | Open workspace overlay |
| <kbd>Ctrl+S</kbd> | Toggle mouse mode (scroll ↔ text select) |

## Sidebar pane

| Key | Action |
|-----|--------|
| <kbd>↑</kbd> / <kbd>k</kbd> | Move selection up |
| <kbd>↓</kbd> / <kbd>j</kbd> | Move selection down |
| <kbd>Enter</kbd> / <kbd>Space</kbd> / <kbd>→</kbd> / <kbd>Tab</kbd> | Focus terminal pane |
| <kbd>Esc</kbd> / <kbd>b</kbd> / <kbd>Ctrl+B</kbd> | Jump back to last session, focus terminal |
| <kbd>n</kbd> | Enter create mode |
| <kbd>r</kbd> | Rename selected session |
| <kbd>d</kbd> | Delete selected session (prompts for confirmation) |
| <kbd>m</kbd> | Move session to a different workspace |
| <kbd>w</kbd> / <kbd>Ctrl+W</kbd> | Open workspace overlay |
| <kbd>q</kbd> | Quit (prompts for confirmation) |

## Create mode

After pressing <kbd>n</kbd> (or <kbd>Ctrl+N</kbd> from the terminal):

| Key | Action |
|-----|--------|
| <kbd>t</kbd> | New terminal session |
| <kbd>a</kbd> | New agent session (runs `claude` on creation) |
| <kbd>Esc</kbd> | Cancel |

### While naming a session

| Key | Action |
|-----|--------|
| <kbd>Enter</kbd> | Create session with current name |
| <kbd>Esc</kbd> | Cancel, discard draft |
| Letters, digits, <kbd>Space</kbd>, `-`, `_`, `.` | Allowed characters |

## Workspace overlay

| Key | Action |
|-----|--------|
| <kbd>↑</kbd> / <kbd>k</kbd> | Move selection up |
| <kbd>↓</kbd> / <kbd>j</kbd> | Move selection down |
| <kbd>Enter</kbd> | Switch to selected workspace |
| <kbd>n</kbd> | Create new workspace |
| <kbd>r</kbd> | Rename selected workspace |
| <kbd>d</kbd> | Delete selected workspace (confirmation required) |
| <kbd>Esc</kbd> | Close overlay, stay in current workspace |
| <kbd>q</kbd> | Quit sidebar-tui |

## Zoom mode

Press <kbd>Ctrl+Z</kbd> to hide the sidebar and give the terminal pane full width. Useful for clean text selection in editors like VS Code.

- <kbd>Ctrl+Z</kbd> again — unzoom, restore sidebar
- <kbd>Ctrl+B</kbd> while zoomed — unzoom and focus sidebar
- <kbd>Ctrl+N</kbd> while zoomed — unzoom and enter create mode
