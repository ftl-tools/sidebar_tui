# Sidebar CLI/TUI

I want a simple TUI for managing terminal sessions in a sidebar in a way that works with my workflow. I want to have sessions grouped into threads on the side bar, and to be able to easily create new ones and switch between them. The full spec is here @eventual_objectives.md but that is too much to start with, so we'll get the basic functionality working first and then iterate from there.

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
- NO E2E or unit tests are skipped, FOR ANY REASON. Install requirements if missing, or do WHATEVER IS REQUIRED to unblock them.

### TUI

- The minimum supported terminal size is 64 characters wide by 24 characters tall.
- Running `sb` should open the TUI.
- The TUI has three components: the sidebar pane, the terminal pane, and the hint bar. They should be laid out like so:
  ```
  ┌──────────────────────────┐┌──────────────────────────────────────────────────────────────────────┐
  │ Sidebar TUI              ││ (base) melchiahmauck@Melchiahs-MacBook-Air sidebar_tui % █           │
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
  ctrl + n New  ctrl + b Focus on sidebar                                       │ ctrl + b -> q Quit
  ```
- On Mac, Windows, and Linux we use `ctrl` for the modifier key. This might change in the future, so below we refer to this as `mod`. In the TUI though we should show the actual keybinding. (Down the road if we vary this based on OS or if we allow users to customize it we whould still show the actual keybinding.)

#### Sidebar Pane

- The Sidebar pane should be a fixed width following the design above.
- There should be one char of padding on the left and right between the session names and the sidebar border.
- The top row should say "Sidebar TUI" in purple text (color 165). It should be left aligned.
- Below the title should be a list of terminal sessions with most recently used at the top.
- The session names should be in white (color 255).
- If a session name is too long to fit in the sidebar it should be wrapped with `│`(s) and `└` characters used to indicate subsequent lines of the same session name. See the example above for reference. These wrapping indicators should be colored slightly darker (color 238) than the session names.
- The background of the selected session should be dark purple (color 54). This highlight should start at the first letter of the session name and stop right before the right sidebar border.
- If there are more sessions than can fit in the sidebar, show a truncation indicator (`...`) at the top and or bottom of the list if extra entries are beyond the visible area.
- If it exists, the top truncation indicator should go directly below the title.
- The truncation indicator should be colored slightly darker (color 238) than the session names.
- When the sidebar is focused it's outline should be lighter (color 250) and when it's not focused it should be darker (color 238).
- The following keybindings should work when the sidebar pane is focused:
  - `enter`, `space`, or `→` - Select: Focus on the terminal pane.
  - `↑` or `k` - Up: Move the selection up one session in the list. If the next row above is the truncation indicator scroll up one and move the selection. If already at the top, do nothing.
  - `↓` or `j` - Down: Move the selection down one session in the list. If the next row below is the truncation indicator scroll down one and move the selection. If already at the bottom, do nothing.
  - `esc` - Jump Back: Select whatever session was selected before the sidebar was focused, and focus on the terminal pane.
  - `n` - New: Enter create mode.
  - `d` - Delete: Show an important confirmation prompt in the hint bar to delete the currently selected session.
    - `y` - Yes: Delete the session and all its data permanently. Focus on the next session in the list. If there is no next session, focus on the previous one. If this was the last session, return to the welcome state.
    - `n` - No: Exit the confirmation prompt. (Focus should remain on the sidebar pane.)
  - `r` - Rename: Start renaming the currently selected session.
    - This should work similarly to drafting a new session in create mode, but instead of an empty name there should be the current session name with the cursor at the end. The user can then backspace and type to change the name. The same character restrictions apply as when drafting a new session.
    - `enter` - Rename: Rename the session to the current name. Exit rename mode and focus on the terminal pane.
    - `esc` - Cancel: Exit rename mode without changing the session name, and return focus to wherever it was before renaming was started.
  - `q` - Quit: Show a confirmation prompt in the hint bar to quit the TUI.
    - `y` or `q` - Yes: Quit the TUI and return to the normal terminal.
    - `n` - No: Exit the confirmation prompt. (Focus should remain on the sidebar pane.)

#### Terminal Pane

- The terminal pane should take up all the remaining space to the right of the sidebar.
- It should show the selected terminal session. This should be a fully functional terminal where I can run commands and see their output. Or even run command line applications like vi.
- There should be one char of padding on the left and right of the terminal pane between the terminal content and the border.
- The terminal text should be white (color 255).
- When the terminal is focused it should have a lighter outline (color 250) and when it's not focused it should have a darker outline (color 238).
- Mouse scrolling when the Sidebar TUI is openned at all, regardless of focus should scroll the terminal pane's visible history.
- When quitting the Sidebar TUI, restarting the computer, and reopening the Sidebar TUI, the terminal sessions should be restored to their previous state as best we can, with comand history, working directory, scrollable visible history, env vars, and anything else we can manage to save and restore.
- The following keybindings should work when the terminal pane is focused:
  - `mod + b` or `mod + t` - Focus on sidebar: Focus on the sidebar pane.
  - `mod + n` - New: Enter create mode.

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
  - The text in the hint bar should be formatted like the example above, with the keybinding in purple (color 165), the description in white (color 255), and two spaces separating each keybinding from the next. The keybindings and descriptions should be left aligned.
  - If the available keybindings are too long to fit on one line they should wrap to multiple lines. The hint bar should grow vertically as needed to accomidate this. A keybinding and its description should never be split across lines.
- The right side of the hint bar should always show the path to quitting the TUI. For example if the terminal pane is focused it should show `mod + b -> q Quit`, because the user must focus on the sidebar pane and then press `q` to quit. Or, if renaming a new session it should show `esc -> q Quit` because the user must stop renaming and then press `q` to quit. This should update dynamically based on the current state of the TUI to always show the correct path to quitting. There should be a separator of `│` colored gray (color 242) just before the quit instructions to separate it from the rest of the hint bar content.
- Sometimes the hint bar might need to show a prompt message along with the keybindings for that prompt. This prompt message should be on the left before any of the keybindings. It should wrap like the keybindings if it's too long to fit on one line, and its text should be colored white (color 255). Generally if we say we want to show a prompt of some sort that should get shown here.
  - Note that it is possible for prompts to have only one keybinding, makeybe something like `k` for "ok".
- Sometimes the hint bar might need to show a temporary message. This should replace the keybindings (but not the quit instructions on the right) and should be colored white (color 255). It should be visible for a few seconds and then disappear and be replaced again by the keybindings.
- Sometimes the message or prompt might need to be emphasized as more important than the keybindings. In this case the background of the hint bar should change to dark red (color 88). Not all prompts and messsages are this important, only some of them.

#### First-time Start Up

If this is the first time starting up the TUI and there are no existing terminal sessions, the terminal pane should be blank and the sidebar pane should be focused. There should be some text in the sidebare pane that says something like "Welcome to Sidebar TUI press `n` to create your first terminal session!" This text should be colored grey (color 238) and should be centered in the sidebar pane. The keybinding in this text should be colored purple (color 165), and should change dynamically if the user changes focus to the empty terminal pane before creating their first session.

## Order

1. First build a hello world TUI.
2. Figure out how on earth agents can automtically test a TUI that requires user input and interaction, in a real Apple terminal environment. Be scrappy and figure it out.
3. Test just the TUI with a moc terminal.
4. Figure out how to do the terminal side of things.
5. Anything else needed.
