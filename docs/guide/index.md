# Quickstart

sidebar-tui is moving toward being a tmux manager, with sessions containing windows and a terminal pane in each window. A persistent window list sits on the left, with the selected terminal pane on the right.

This release aligns terminology; it still uses Sidebar's own PTY server rather than tmux. See [terminology and compatibility](./terminology.md).

## Layout

The 28-column sidebar contains the session title and scrollable window list, followed by a horizontal divider and a column of contextual hints. Hints, confirmations, and temporary messages stay inside the sidebar; the detach/exit path sits below them.

The terminal fills the entire right side, from the first row to the last, without a frame or padding. Changing hints does not resize the terminal or disturb its output. The sidebar outline indicates focus. Zoom hides the sidebar and its hints together; toggle focus to bring them back. Session and help views use the right side while their hints remain on the left.

On short screens, hints are clipped to preserve window-list space and the exit path. Press <kbd>?</kbd> from the sidebar for the full command reference.

## Launch

```sh
sb
```

That's it. On first launch a **Default** session is created and the sidebar is focused, ready for you to create your first window.

## Your first window

1. Press <kbd>c</kbd> to create a window (or <kbd>a</kbd> for a window running Claude).
2. Type a name (e.g. `api-server`) and press <kbd>Enter</kbd>.
3. You're now in its terminal pane. Run whatever you like.

## Switching windows

Focus the sidebar with <kbd>Ctrl+B</kbd>, use <kbd>↑</kbd>/<kbd>↓</kbd> (or <kbd>k</kbd>/<kbd>j</kbd>) to select a window, and press <kbd>Enter</kbd> to focus it.

Pressing <kbd>Ctrl+B</kbd> while in the sidebar also commits the highlighted window. <kbd>Esc</kbd> cancels browsing; <kbd>l</kbd> switches to the last active window.

## What's next

- [Installation options](/guide/installation) — Homebrew, npm, curl, AUR
- [All keybindings](/guide/keybindings) — complete reference
- [Sessions](/guide/sessions) — group windows by project
- [Proposed tmux migration plan](./tmux_migration_plan.md) — incremental, testable steps toward a native tmux manager
