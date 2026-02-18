---
name: tui-debugging
description: >
  Use tmux to manually spawn, interact with, and inspect the sidebar TUI (sb binary) for
  debugging and manual verification. Use this skill when you need to test TUI behavior,
  verify rendering, debug layout issues, or manually exercise features of the sb binary
  without writing E2E tests.
---

# TUI Debugging with tmux

Use tmux to run `sb` in a controlled session, send keystrokes, and capture the rendered screen as plain text.

## Spawn sb

```bash
tmux new-session -d -s sb-test -x 220 -y 50
tmux send-keys -t sb-test "sb" Enter
sleep 1.5
```

- `-x 220 -y 50` sets terminal dimensions (width x height)
- Use `sb -s <name>` to open with a specific session name

## Capture the Screen

```bash
tmux capture-pane -t sb-test -p
```

Returns the full rendered screen as plain text. Colors/styling are stripped but layout and content are accurate.

## Send Keystrokes

```bash
# Regular character (no trailing Enter)
tmux send-keys -t sb-test "n" ""

# Enter
tmux send-keys -t sb-test "" Enter

# Control keys
tmux send-keys -t sb-test "C-b" ""   # Ctrl+B
tmux send-keys -t sb-test "C-q" ""   # Ctrl+Q
tmux send-keys -t sb-test "C-n" ""   # Ctrl+N

# Special keys
tmux send-keys -t sb-test "Escape" ""
tmux send-keys -t sb-test "Tab" ""
tmux send-keys -t sb-test "Space" ""
tmux send-keys -t sb-test "BSpace" ""  # Backspace

# Arrow keys
tmux send-keys -t sb-test "Down" ""
tmux send-keys -t sb-test "Up" ""
tmux send-keys -t sb-test "Left" ""
tmux send-keys -t sb-test "Right" ""
```

## Typical Debugging Workflow

```bash
# 1. Kill any leftover session
tmux kill-session -t sb-test 2>/dev/null

# 2. Spawn fresh
tmux new-session -d -s sb-test -x 220 -y 50
tmux send-keys -t sb-test "sb" Enter
sleep 1.5

# 3. Capture initial render
tmux capture-pane -t sb-test -p

# 4. Interact (example: focus sidebar, navigate down, capture)
tmux send-keys -t sb-test "C-b" ""
sleep 0.3
tmux send-keys -t sb-test "Down" ""
sleep 0.3
tmux capture-pane -t sb-test -p

# 5. Cleanup
tmux kill-session -t sb-test
```

## Tips

- Always `sleep` after sending keys to give the TUI time to re-render before capturing
- Use `sb list` / `sb kill <name>` to inspect and manage daemon sessions independently of tmux
- Capture output is 220 chars wide — search for specific strings to verify content at known positions
- To test sb subcommands non-interactively, run them directly via Bash (e.g. `sb list`, `sb kill foo`)
