# Scratchpad

- I'm still seeing two chars of padding on the sides of - Claude code has weird formatting. It shows the old input box graphic above the final result. I assume this is caused by the way claude code runs in the terminal and not in a typical, full-screen TUI.
- I can't scroll when there is stuff updating in the terminal. For example claude code runs in the terminal (not in a typical, full-screen TUI), it has an animating thing when it is working, and I can scroll up to see anything. I presume this is because when claude code updates the terminal, it resets the scroll position, or something like that.
- I can't use a mouse to copy paste out of the terminal.
- Auto scroll down to new content is too slow. It should be super fast or instant.
- `esc` jump back is changing the selected session in the sidebar pane, but doesn't change the terminal session in the terminal pane.
- When I swap terminals in the sidebar but don't change the focus yet, it doesn't show the live terminal. It shows the last content that was in there, but if things have changed it doesn't show the live content.
- As I type a really long terminal session name it does not wrap until I hit enter. It should wrap as I type.
- Think about how to auto-name claude sessions.
- Show one symbol when a command is actively running, and a background animation when something is currently or has just recently been animating in the terminal. This way I can see at a glance which sessions are active, and which ones have recently been active.
- Unless a session has been explicitly named, it should show the file location and currently running command. We need a way to differentiate multiple with the same name.
- The app crashes if the window get's too narrow.
- We need a better option than last accessed on top. Since I keep forgetting the order.
- Use Omachy tmux hotkeys by default.
