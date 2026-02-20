# sidebar-tui hero demo
# Run: vhs docs/demo.tape
#
# Requirements:
#   brew install vhs   (installs vhs + ttyd + ffmpeg + Chrome)
#
# Outputs MP4 + WebM into docs/public/hero/ for the VitePress hero slot.
# The tape is deterministic — re-run at any time to re-record.

Output docs/public/hero/demo.mp4
Output docs/public/hero/demo.webm

# Terminal appearance
Set Shell "zsh"
Set FontFamily "JetBrains Mono"
Set FontSize 16
Set Width 1100
Set Height 650
Set Theme "Catppuccin Mocha"
Set WindowBar Colorful
Set WindowBarSize 32
Set BorderRadius 10
Set Padding 18
Set Margin 24
Set MarginFill "#0c0c0f"
Set TypingSpeed 60ms
Set Framerate 30
Set PlaybackSpeed 1.0
Set LoopOffset 1%

# --- Setup (hidden from recording) ---
Hide
Type "clear"
Enter
Sleep 200ms
Show

# --- Demo: launch sidebar-tui ---
Type "sb"
Sleep 400ms
Enter
Sleep 2s

# sidebar is focused — create first session
Type "n"
Sleep 400ms
Type "t"
Sleep 300ms
Type "api-server"
Enter
Sleep 1.5s

# we're now in the terminal pane — run a command
Type "git status"
Enter
Sleep 1.5s

# go back to sidebar and create another session
Ctrl+B
Sleep 500ms
Type "n"
Sleep 300ms
Type "t"
Sleep 300ms
Type "docs"
Enter
Sleep 1.2s

# run something in second session
Type "echo hello from docs"
Enter
Sleep 1s

# switch back to api-server via sidebar
Ctrl+B
Sleep 400ms
Down
Sleep 400ms
Enter
Sleep 1s

# zoom terminal pane
Ctrl+Z
Sleep 2s

# unzoom
Ctrl+Z
Sleep 1.5s

# quit
Ctrl+B
Sleep 400ms
Type "q"
Sleep 500ms
Type "y"
Sleep 800ms
