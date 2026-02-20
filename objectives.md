# Sidebar CLI/TUI

I want a simple TUI for managing terminal sessions in a sidebar in a way that works with my workflow. I want to have sessions grouped into threads on the side bar, and to be able to easily create new ones and switch between them. The full spec is here @eventual_objectives.md but that is too much to start with, so we'll get the basic functionality working first and then iterate from there.

## IMPORTANT

If you make a fix or an update that is not reflected or flat out differenet in the spec below, then update the spec to match.

## Spec

The general requirements are as follows:

- You MUST use rust. Research and use any other tools that are needed to accomplish the objectives.
- There are several OpenSource, example projects that have been cloned into references that prove out similar functionality to what we want using similar tech. Use these as references to figure out how to build the TUI and terminal functionality we need.
- You must create at the very least the following E2E tests. They must all work in the Apple terminal on my computer:
  - You have used the Sidebar TUI and it's layout matches the spec above exactly.
  - You can run `git status` in the TUI terminal and it has the same output as running `git status` in a normal terminal.
  - You can run `vi` in the TUI terminal, open a file, edit it, save it, and exit, and the file is changed as expected.
  - In the TUI terminal, you can start typing `git status`, backspace before you send it, type `echo "hello world"`, send that, and see the expected output in the TUI terminal.
  - There must be at least one E2E test that works on the real Sidebar TUI in the Apple terminal for each individual bullet point in this spec.
  - There must be E2E tests covering: creating a workspace, switching between workspaces (verifying sessions are isolated per workspace), renaming a workspace, deleting a workspace (verifying its sessions are gone), moving a session between workspaces, and verifying workspace state is restored after switching away and back.
- NO E2E or unit tests are skipped, FOR ANY REASON. Install requirements if missing, or do WHATEVER IS REQUIRED to unblock them.

### Workspaces

A workspace is a named container that groups terminal sessions and saves view/layout state. Every session belongs to exactly one workspace. Workspaces allow you to organize sessions by project, context, or any other grouping that makes sense to you.

Key properties of workspaces:

- Every session belongs to exactly one workspace. Sessions cannot be shared across workspaces.
- Background sessions in other workspaces keep running when you switch — they are just hidden.
- Each workspace saves its full view state: which session was last selected, which pane was focused, scroll position of the sidebar list, and scroll position of each session's terminal history. This state is restored when you switch back to the workspace.
- Workspaces persist across restarts, including all saved state.
- There must always be at least one workspace. If deleting a workspace would leave none, a new "Default" workspace is auto-created.
- New sessions are always created in the currently active workspace.

### TUI

- The minimum supported terminal size is 64 characters wide by 24 characters tall.
- Running `sb` should open the TUI.
- The TUI has three components: the sidebar pane, the terminal pane, and the hint bar. They should be laid out like so:
  ```
  ┌──────────────────────────┐┌──────────────────────────────────────────────────────────────────────┐
  │ WorkspaceName            ││ (base) melchiahmauck@Melchiahs-MacBook-Air sidebar_tui % █           │
  │ ...                      ││                                                                      │
  │ Name of Terminal Session ││                                                                      │
  │ Really, really long name ││                                                                      │
  │ │for this specific       ││                                                                      │
  │ └terminal session.       ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ Terminal Session         ││                                                                      │
  │ ...                      ││                                                                      │
  └──────────────────────────┘└──────────────────────────────────────────────────────────────────────┘
   ctrl + n New  ctrl + b Sidebar                                                │ ctrl + b -> q Quit
  ```
- On Mac, Windows, and Linux we use `ctrl` for the modifier key. This might change in the future, so below we refer to this as `mod`. In the TUI though we should show the actual keybinding. (Down the road if we vary this based on OS or if we allow users to customize it we whould still show the actual keybinding.)

#### Sidebar Pane

- The Sidebar pane should be a fixed width following the design above.
- There should be one char of padding on the left and right between the session names and the sidebar border.
- The top row should show the current workspace name in purple text (color 99). It should be left aligned. If the workspace name is too long to fit, it should be truncated with `...` at the end.
- Below the title should be a list of terminal sessions with most recently used at the top.
- The session names should be in white (color 255).
- If a session name is too long to fit in the sidebar it should be wrapped with `│`(s) and `└` characters used to indicate subsequent lines of the same session name. See the example above for reference. These wrapping indicators should be colored slightly darker (color 238) than the session names.
- The background of the selected session should be dark grey (color 238). This highlight should start at the first letter of the session name and stop right before the right sidebar border.
- If there are more sessions than can fit in the sidebar, show a truncation indicator (`...`) at the top and or bottom of the list if extra entries are beyond the visible area.
- If it exists, the top truncation indicator should go directly below the title.
- The truncation indicator should be colored slightly darker (color 238) than the session names.
- When the sidebar is focused it's outline should be purple (color 99) and when it's not focused it should be darker (color 238).
- The following keybindings should work when the sidebar pane is focused:
  - `enter`, `space`, `→`, or `tab` - Select: Focus on the terminal pane.
  - `↑` or `k` - Up: Move the selection up one session in the list. If the next row above is the truncation indicator scroll up one and move the selection. If already at the top, do nothing.
  - `↓` or `j` - Down: Move the selection down one session in the list. If the next row below is the truncation indicator scroll down one and move the selection. If already at the bottom, do nothing.
  - `esc`, `b`, `mod + b`, or `mod + t` - Jump Back: Select whatever session was selected before the sidebar was focused, and focus on the terminal pane.
  - `n` - New: Enter create mode.
  - `d` - Delete: Show an important confirmation prompt in the hint bar to delete the currently selected session.
    - `y` - Yes: Delete the session and all its data permanently. Focus on the next session in the list. If there is no next session, focus on the previous one. If this was the last session, return to the welcome state.
    - `n` - No: Exit the confirmation prompt. (Focus should remain on the sidebar pane.)
  - `r` - Rename: Start renaming the currently selected session.
    - This should work similarly to drafting a new session in create mode, but instead of an empty name there should be the current session name with the cursor at the end. The user can then backspace and type to change the name. The same character restrictions apply as when drafting a new session.
    - `enter` - Rename: Rename the session to the current name. Exit rename mode and focus on the terminal pane.
    - `esc` - Cancel: Exit rename mode without changing the session name, and return focus to wherever it was before renaming was started.
  - `m` - Move: Open the workspace overlay in "move to workspace" mode. The user selects a destination workspace and the currently selected session is moved there. See the Workspace Overlay section for details.
  - `w` or `mod + w` - Workspaces: Open the workspace overlay. (`w` works from the sidebar pane; `mod + w` works from any pane.)
  - `q` - Quit: Show a confirmation prompt in the hint bar to quit the TUI.
    - `y` or `q` - Yes: Quit the TUI and return to the normal terminal.
    - `n` - No: Exit the confirmation prompt. (Focus should remain on the sidebar pane.)

#### Terminal Pane

- The terminal pane should take up all the remaining space to the right of the sidebar.
- It should show the selected terminal session. This should be a fully functional terminal where I can run commands and see their output. Or even run command line applications like vi.
- There should be one char of padding on the left and right of the terminal pane between the terminal content and the border.
- The terminal text should be white (color 255).
- When the terminal is focused it should have a purple outline (color 99) and when it's not focused it should have a darker outline (color 238).
- Mouse scrolling when the Sidebar TUI is openned at all, regardless of focus should scroll the terminal pane's visible history. NOTE: This only works when mouse mode is enabled (see `mod + s` below).
- By default, mouse mode is disabled to allow native terminal text selection (for copying text). Use `mod + s` to toggle between text selection mode and mouse scroll mode.
- When quitting the Sidebar TUI, restarting the computer, and reopening the Sidebar TUI, the terminal sessions should be restored to their previous state as best we can, with comand history, working directory, scrollable visible history, env vars, and anything else we can manage to save and restore.
- The following keybindings should work when the terminal pane is focused:
  - `mod + b` or `mod + t` - Sidebar: Focus on the sidebar pane.
  - `mod + n` - New: Enter create mode.
  - `mod + s` - Toggle mouse mode: Toggle between text selection mode (native terminal selection works) and mouse scroll mode (scroll wheel works but text selection is blocked). The hint bar shows the current mode ("Text select" or "Mouse scroll").
  - `mod + z` - Zoom: Toggle zoom mode. When zoomed, the sidebar is hidden and the terminal pane takes the full width of the TUI. This allows clean text selection of only terminal content (useful in editors like VSCode where selection otherwise includes sidebar borders and session names). Press `mod + z` again to unzoom (restore the sidebar). Pressing `mod + b` while zoomed also unzooms and focuses the sidebar. Entering create mode while zoomed also unzooms automatically. A timed hint bar message confirms the current state.
  - `mod + w` - Workspaces: Open the workspace overlay. (This keybinding works from any pane.)

#### Create Mode

- When in create mode, the hint bar should show the possible things to create:
  - `t` - Terminal Session: Start drafting a new terminal session.
  - `a` - Agent Session: Start drafting a new terminal session. If it is created run `claude` in it before focusing on it.
  - Later on we will support user customizable templates for different types of sessions.
  - `esc` - Cancel: Exit create mode, and return focus to wherever it was before entering create mode.
- If focus was on the terminal when create mode was entered, focus should not change until drafting a new session is started.
- When drafting a new session:
  - Add an empty session row to the top of the sidebar list.
  - This new row should be selected and focused.
  - There should be a blinking `|` cursor at the start of the session name to indicate that the user can type there.
  - The user should be allowed to type uppercase and lowercase letters, numbers, spaces, and the following special characters: `-`, `_`, and `.`. Any other characters should be ignored.
  - The terminal pane should be blank durring this time and should not be interactive. The user should not be able to focus on it or type in it.
  - The following keybindings should work when drafting a new session:
    - `enter` - Create: Create the new session with the current name. Focus on the terminal pane. The new session should be created in the working directory that this Sidebar TUI instance was launched from, and should have the same env vars as the terminal that launched this Sidebar TUI. If multiple Sidebar TUI instances are openned they they might each have different working directories and env vars, and the sessions they create should reflect that, even though all terminal sessions, once created, are shared between Sidebar TUI instances.
    - `esc` - Cancel: Remove the new, draft session row from the sidebar list and exit create mode. This should return focus to wherever it was before entering create mode.
- Obviously other keybindings should not work when in create or draft mode. Only the ones listed above.

#### Hint Bar

- The whole bottom row(s) of the TUI should be a hint bar.
- The background of the hint bar should be dark grey (color 238).
- The hint bar should almost always show the currently available keybindings and actions given the current context.
  - The text in the hint bar should be formatted like the example above, with the keybinding in purple (color 99), the description in white (color 255), and two spaces separating each keybinding from the next. The keybindings and descriptions should be left aligned.
  - If the available keybindings are too long to fit on one line they should wrap to multiple lines. The hint bar should grow vertically as needed to accomidate this. A keybinding and its description should never be split across lines.
- The right side of the hint bar should always show the path to quitting the TUI. For example if the terminal pane is focused it should show `mod + b -> q Quit`, because the user must focus on the sidebar pane and then press `q` to quit. Or, if renaming a new session it should show `esc -> q Quit` because the user must stop renaming and then press `q` to quit. This should update dynamically based on the current state of the TUI to always show the correct path to quitting. There should be a separator of `│` colored gray (color 242) just before the quit instructions to separate it from the rest of the hint bar content.
- Sometimes the hint bar might need to show a prompt message along with the keybindings for that prompt. This prompt message should be on the left before any of the keybindings. It should wrap like the keybindings if it's too long to fit on one line, and its text should be colored white (color 255). Generally if we say we want to show a prompt of some sort that should get shown here.
  - Note that it is possible for prompts to have only one keybinding, makeybe something like `k` for "ok".
- Sometimes the hint bar might need to show a temporary message. This should replace the keybindings (but not the quit instructions on the right) and should be colored white (color 255). It should be visible for a few seconds and then disappear and be replaced again by the keybindings.
- Sometimes the message or prompt might need to be emphasized as more important than the keybindings. In this case the background of the hint bar should change to dark red (color 88). Not all prompts and messsages are this important, only some of them.

#### First-time Start Up

On the very first launch (no workspaces or sessions exist), a workspace named "Default" is automatically created and made active. The terminal pane should be blank and the sidebar pane should be focused. There should be some text in the sidebar pane that says something like "Welcome to Sidebar TUI press `n` to create your first terminal session!" This text should be colored grey (color 238) and should be centered in the sidebar pane. The keybinding in this text should be colored purple (color 99), and should change dynamically if the user changes focus to the empty terminal pane before creating their first session.

#### Workspace Overlay

The workspace overlay is a full-screen view that replaces the sidebar and terminal panes when `mod + w` is pressed from any pane. It is used to view, switch, create, rename, and delete workspaces. The hint bar remains visible at the bottom.

**Layout:**

The overlay covers the entire main area (sidebar + terminal panes area). It shows:

- A title row at the top: "Workspaces" in purple text (color 99), left aligned with one char of left padding.
- A list of all workspaces below the title, one per row, in white (color 255). The currently active workspace is marked with a `*` indicator to the left of its name.
- The selected/highlighted workspace row has a dark grey background (color 238), same as selected sessions in the sidebar.
- If the list is too long to fit, truncation indicators (`...`) appear at the top and/or bottom, same as in the sidebar session list.
- The hint bar at the bottom shows the available keybindings for the overlay (see below).

**Normal mode keybindings:**

- `↑` / `k` - Up: Move the selection up one workspace.
- `↓` / `j` - Down: Move the selection down one workspace.
- `enter` - Switch: Switch to the selected workspace. Close the overlay and restore the workspace's last saved state (last focused pane, last selected session, scroll positions, etc.).
- `n` - New: Start drafting a new workspace inline. An empty row appears at the top of the list with a blinking `|` cursor. The same character restrictions apply as when drafting a new session. Press `enter` to create or `esc` to cancel.
- `r` - Rename: Start renaming the selected workspace inline. The current name is shown with the cursor at the end. Press `enter` to confirm or `esc` to cancel.
- `d` - Delete: Show an important (dark red background, color 88) confirmation prompt in the hint bar: "Delete workspace and ALL its sessions permanently?" with `y` to confirm and `n` to cancel. If confirmed, the workspace and all of its sessions are permanently deleted. If the deleted workspace was the active one, switch to the first remaining workspace. If there are no remaining workspaces, auto-create a new "Default" workspace.
- `esc` - Close: Close the overlay and return to the TUI without switching workspaces.
- `q` - Quit: Show the quit confirmation prompt in the hint bar, same as when on the sidebar pane.

**Move-to-workspace mode:**

When triggered by pressing `m` in the sidebar, the workspace overlay opens in move mode. The title row changes to "Move to Workspace" in purple. The behavior is identical to normal mode except:

- `enter` - Move: Move the currently selected session (from the sidebar) to the highlighted workspace. If the selected workspace is the current workspace, do nothing. Close the overlay and return focus to the sidebar.
- `esc` - Cancel: Close the overlay and return focus to the sidebar without moving anything.
- Creating, renaming, and deleting workspaces are not available in move mode.

**Quit path hint:**

When the workspace overlay is open, the right side of the hint bar should show `q Quit` as the quit path (since `q` works directly from the overlay).

**Persistence:**

The workspace's saved state (last focused pane, last selected session, scroll position of each session's terminal history, sidebar scroll position) is persisted to disk and restored when the TUI is reopened, same as sessions.

## Distribution

### Releases
- Feature branches → PRs → `main`; CI runs tests on every PR
- Release by pushing a version tag: `git tag v0.2.0 && git push --tags`
- GitHub Actions builds all platform binaries, creates a GitHub Release, publishes to npm

### Updates
- Binary checks GitHub Releases API once per day (cached) and self-updates on next run (`self_update` crate)
- If installed via Homebrew, skip self-update and print: `"sb v0.2.0 available — run 'brew upgrade sb' to update"`

### Windows
- Daemon IPC: TCP on localhost (`127.0.0.1:PORT`, port stored in a lockfile) replacing Unix socket
- Supported from initial release

### Install methods
| Method | Command |
|---|---|
| curl | `curl -fsSL https://... \| bash` |
| Homebrew | `brew install <tap>/sb` |
| npm | `npm install -g sidebar-tui` |
| bun | `bun add -g sidebar-tui` |
| AUR | `paru -S sidebar-tui` |

### One-time manual setup (human required)
| Task | Why it can't be automated |
|---|---|
| Make the GitHub repo public | Requires GitHub account action |
| Create an npm account + generate publish token | External account |
| Create an AUR account + upload SSH public key | External account, SSH key pair |
| Create `ftl-tools/homebrew-sidebar-tui` GitHub repo (the Homebrew tap) | Requires GitHub account action; the repo must exist before CI can push to it |
| Create a GitHub PAT and add secrets to repo settings | `NPM_TOKEN`, `AUR_SSH_PRIVATE_KEY`, `AUR_USERNAME`, `AUR_EMAIL`, `HOMEBREW_TAP_GITHUB_TOKEN` |

## Order

1. First build a hello world TUI.
2. Figure out how on earth agents can automtically test a TUI that requires user input and interaction, in a real Apple terminal environment. Be scrappy and figure it out.
3. Test just the TUI with a moc terminal.
4. Figure out how to do the terminal side of things.
5. Anything else needed.
