# Quick Start

sidebar-tui is a terminal session manager that lives in a sidebar inside your terminal. You get a persistent list of named sessions on the left and a full terminal on the right — no window switching, no context loss.

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
