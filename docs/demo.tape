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
# Width/Height must be even for H.264; nearest even values to requested 1125x975
Set Width 1126
Set Height 750
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
# The old demo shared the user's server; isolate it so recording cannot touch real windows.
Type "export SB_DEMO_ROOT=$(mktemp -d /tmp/sb_demo_XXXXXX); export XDG_DATA_HOME=$SB_DEMO_ROOT/data XDG_RUNTIME_DIR=$SB_DEMO_ROOT/run; mkdir -p $XDG_DATA_HOME $XDG_RUNTIME_DIR"
Enter
Sleep 300ms
Type "clear"
Enter
Sleep 200ms
Show

# --- Demo: launch sidebar-tui ---
Type "sb"
Sleep 400ms
Enter
Sleep 2s

# sidebar is focused — create first window
Type "c"
Sleep 400ms
Type "api-server"
Enter
Sleep 1.5s

# we're now in the terminal pane — run a command
Type "echo API server ready"
Enter
Sleep 1.5s

# go back to sidebar and create another window
Ctrl+B
Sleep 500ms
Type "c"
Sleep 300ms
Type "docs"
Enter
Sleep 1.2s

# run something in second window
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

# show the session chooser
Ctrl+B
Sleep 400ms
Type "s"
Sleep 1.5s
Escape
Sleep 400ms

# zoom terminal pane
Type "z"
Sleep 2s

# unzoom and detach (windows keep running)
Ctrl+B
Sleep 400ms
Type "d"
Sleep 500ms
Type "y"
Sleep 800ms

Hide
Type "sb shutdown; rm -rf $SB_DEMO_ROOT"
Enter
Sleep 300ms
