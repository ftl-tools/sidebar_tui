# Quickstart

sidebar-tui is a terminal session manager that lives in a sidebar inside your terminal. You get a persistent list of named sessions on the left and a full terminal on the right — no window switching, no context loss.

## Layout

The 28-column sidebar contains the workspace title and scrollable session list, followed by a horizontal divider and a column of contextual hints. Hints, confirmations, and temporary messages stay inside the sidebar; the detach/exit path sits below them.

The terminal fills the entire right side, from the first row to the last, without a frame or padding. Changing hints does not resize the terminal or disturb its output. The sidebar outline indicates focus. Zoom hides the sidebar and its hints together; toggle focus to bring them back. Workspace and help views use the right side while their hints remain on the left.

On short screens, hints are clipped to preserve session-list space and the exit path. Press <kbd>?</kbd> from the sidebar for the full command reference.

## Launch

```sh
sb
```

That's it. On first launch a **Default** workspace is created and the sidebar is focused, ready for you to create your first session.

## Your first session

1. Press <kbd>n</kbd> to enter create mode.
2. Press <kbd>t</kbd> to start a new terminal session.
3. Type a name (e.g. `api-server`) and press <kbd>Enter</kbd>.
4. You're now in a full terminal. Run whatever you like.

## Switching sessions

Focus the sidebar with <kbd>Ctrl+B</kbd>, use <kbd>↑</kbd>/<kbd>↓</kbd> (or <kbd>k</kbd>/<kbd>j</kbd>) to select a session, and press <kbd>Enter</kbd> to focus it.

Or press <kbd>Ctrl+B</kbd> while already in the sidebar to jump back to the last active session.

## What's next

- [Installation options](/guide/installation) — Homebrew, npm, curl, AUR
- [All keybindings](/guide/keybindings) — complete reference
- [Workspaces](/guide/workspaces) — group sessions by project
