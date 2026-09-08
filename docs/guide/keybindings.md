# Keybindings

Sidebar uses its visible sidebar as a persistent tmux-style command mode. Contextual hints appear as a column below the session list, separated by a horizontal line—not under the terminal. The sidebar outline indicates focus; the terminal itself is unframed.

## Toggle and direct shortcuts

| Key | Action |
|-----|--------|
| <kbd>Ctrl+Space</kbd> / <kbd>Ctrl+B</kbd> | Toggle between terminal and sidebar; toggling from the sidebar commits the highlighted window |
| <kbd>Cmd+Space</kbd> / <kbd>Cmd+B</kbd> | Toggle when the OS and terminal expose the Command chord |
| <kbd>Alt+1</kbd>…<kbd>Alt+9</kbd> | Switch directly to a displayed window position |
| <kbd>Alt+Left</kbd> / <kbd>Alt+Right</kbd> | Previous/next window |
| <kbd>Alt+Up</kbd> / <kbd>Alt+Down</kbd> | Previous/next workspace |
| <kbd>Alt+Shift+Left</kbd> / <kbd>Alt+Shift+Right</kbd> | Reorder the highlighted window |

Direct Alt shortcuts also work while the sidebar is focused, but window shortcuts only update the highlighted preview there. Modal naming and confirmation fields take precedence over every global binding.

## Sidebar commands

| Key | Action |
|-----|--------|
| <kbd>↑</kbd> / <kbd>k</kbd>, <kbd>↓</kbd> / <kbd>j</kbd> | Browse windows with live preview |
| <kbd>Enter</kbd> / Toggle | Commit highlighted window and focus its terminal |
| <kbd>Esc</kbd> / <kbd>q</kbd> | Cancel browsing and restore the pre-browse window |
| <kbd>1</kbd>…<kbd>9</kbd> | Highlight a displayed window position; Enter commits |
| <kbd>n</kbd> / <kbd>p</kbd> | Highlight next/previous window |
| <kbd>l</kbd> | Switch to the last active window |
| <kbd>c</kbd> / <kbd>a</kbd> | Create a terminal/agent; type an optional name and press Enter |
| <kbd>r</kbd> / <kbd>,</kbd> | Rename highlighted window |
| <kbd>&</kbd> / <kbd>Delete</kbd> | Delete highlighted window after confirmation |
| <kbd>m</kbd> | Move highlighted window to another workspace |
| <kbd>s</kbd> | Open workspace chooser |
| <kbd>C</kbd> / <kbd>R</kbd> / <kbd>K</kbd> | Create, rename, or delete the current workspace |
| <kbd>P</kbd> / <kbd>N</kbd> | Previous/next workspace |
| <kbd>z</kbd> | Hide sidebar and focus terminal |
| <kbd>S</kbd> | Toggle mouse scrolling/text selection |
| <kbd>d</kbd> | Detach after confirmation, leaving terminals running |
| <kbd>?</kbd> | Show command help |

Unsupported sidebar keys are consumed and are never forwarded to the child terminal.

## Workspace chooser

Use arrows or <kbd>j</kbd>/<kbd>k</kbd> and <kbd>Enter</kbd> to select. <kbd>Esc</kbd> or <kbd>q</kbd> closes the chooser. <kbd>C</kbd>, <kbd>R</kbd>/<kbd>$</kbd>, and <kbd>K</kbd> create, rename, and delete workspaces.

## Naming and confirmation

Naming fields accept letters, digits, spaces, `-`, `_`, and `.`. Enter submits and Esc cancels. An empty terminal/agent name generates a unique name automatically. Destructive actions always require confirmation.

The retired global shortcuts <kbd>Ctrl+N</kbd>, <kbd>Ctrl+W</kbd>, <kbd>Ctrl+S</kbd>, <kbd>Ctrl+Z</kbd>, <kbd>Ctrl+Q</kbd>, and <kbd>Ctrl+T</kbd> now pass through to terminal applications.
