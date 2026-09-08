# Sidebar TUI

Future design vocabulary follows tmux: **sessions → windows → panes**. The former proposed "threads" are windows and their terminals are panes. This is a future design, not implemented behavior: today each window has one pane, and Sidebar still uses its own PTY server. Older hotkey and persistence proposals below are not the current contract; see [the guide](docs/guide/index.md).

I want a simple TUI for managing terminal panes in a sidebar in a way that works with my workflow. I want to have panes grouped into windows on the side bar, and to be able to easily create new ones and switch between them.

## Spec

Sidebar TUI should be a TUI that opens when I run `sb` in the terminal. The TUI should have a Sidebar View on the left, a Terminal View on the right, and a contextual hints banner across the bottom.

### Session Header

At the very top of the sidebar view, the current session name should be displayed prominently. The background of this header should be the session's primary color, making it immediately clear which session is active. The text should be black (or white if the primary color is too dark) for contrast. This header should span the full width of the sidebar.

### Sidebar View

The sidebar view should be a fixed width. It should contain a list of window names and have a slightly lighter background than the terminal view. The pane names of the active window should be listed under it. None of the other panes should be visible. The background of the active window name and all its pane names should be highlighted a little lighter. The name of the active pane should be the primary color. This way users can easily see which window and pane they are in. No names should be truncated, just wrapped to the next line if they are too long.

Pinned windows are always visible at the top of the sidebar, the rest of the windows are listed below in order of most recently used. A primary colored separator should divide the pinned windows from the unpinned windows. Pinned panes are always visible at the top of their window's pane list, the rest of the panes are listed below in order of most recently used. A text colored separator should divide the pinned panes from the unpinned panes within each window.

#### Hotkeys

Our primary mod key is `ctrl` on Mac and `alt` on linux and windows. We refer to this key below as `mod`.

When in pane focus mode all input is sent to the terminal view, but the following hotkeys will swap control to the sidebar:

- `mod + b` or `mod + s`: Focus on the active pane of the active window in the sidebar.
- `mod + t`: Focus on the active window in the sidebar.
- `mod + w`: Open the Sessions View to switch sessions or manage them. The current session will be pre-focused.
- `mod + n`: Same as the `n` option when focused on the sidebar, but lets the user jump straight to creating a new item. If they cancel, then focus should be returned to the terminal view, not the sidebar.

When in pane focus mode:

- `up arrow`: Swap the above pane to be the active pane. If this is the top pane in the window don't do anything.
- `down arrow`: Swap the below pane to be the active pane. If this is the bottom pane in the window don't do anything.
- `left arrow`: Focus on the active window.
- `right arrow` or `enter`: Swap control back to the terminal view of the currently selected pane.
- `esc`: Swap back to the original active pane and swap control back to the terminal view.

When in window focus mode:

- `up arrow`: Swap the above window to be the active window. If this is the top window don't do anything. When swapping the active window, the active pane should be set to whatever pane was active the last time this window was active. If this window has never been active, the active pane should be set to the top pane in the window.
- `down arrow`: Swap the below window to be the active window. If this is the bottom window don't do anything. When swapping the active window, the active pane should be set to whatever pane was active the last time this window was active. If this window has never been active, the active pane should be set to the top pane in the window.
- `right arrow` or `enter`: Focus on the active pane of the active window.

When in either window or pane focus mode:

- `w`: Open the Sessions View to switch to a different session or manage sessions. The current session will be pre-focused in the list.
- `esc`: Swap back to the originally active window and pane from before the focus was swapped to the sidebar, and swap control back to the terminal view.
- `b`: Swap between window focus mode and pane focus mode. (This feature might get axed.)
- `p`: Pin the currently focused window or pane. (Stay focused on the item that was just pinned, and show a hint message that the item is now pinned.)
- `u`: Unpin the currently focused window or pane. (Stay focused on the item that was just unpinned, and show a hint message that the item is now unpinned.)
- `d`: Highlight the background of the current sesson or window (including all its panes) red to indicate that it is being marked for deletion. The hint banner's background should become red to indicate that the user is in delete mode.
  - `y`: Delete the currently highlighted window or pane. If a window is deleted, all its panes are also deleted. If the active window or pane is deleted, the active window should become the most recently used window that is still open, and the active pane should become the most recently used pane in that window that is still open.
  - `n`: Cancel deletion and remove the red highlight.
- `r`: Rename the currently focused window or pane. The name should become editable with a blinking cursor. The user can type the new name, and upper and lower case letters, numbers, spaces, hyphens, and underscores should be allowed, all other characters should be ignored.
  - `enter`: Save the new name and exit rename mode.
  - `esc`: Cancel renaming and revert to the original name, then exit rename mode.
- `n`: Give users the option of creating a new item.
  - `t`: Start drafting a new window. It should add a new entry for a window name at the top of the unpinned window list with a blinking cursor, indicating that the user is drafting a new window name. They can type the name of the new window. Upper and lower case letters, numbers, spaces, hyphens, and underscores should be allowed, all other characters should be ignored.
    - `enter`: Create the new window. There will be no active pane. The user can use the hotkeys to create the kind of pane they want.
    - `esc`: Cancel drafting the new window and focus on whatever window or pane had focus before.
  - `s`: Start drafting a new terminal pane in the active window. It should add a new entry for a pane name at the top of the window's list of panes with a blinking cursor, indicating that the user is drafting a new pane name. They can type the name of the new pane. Upper and lower case letters, numbers, spaces, hyphens, and underscores should be allowed, all other characters should be ignored.
    - `enter`: Create the new pane and swap control to it in the terminal view.
    - `esc`: Cancel drafting the new pane and focus on whatever window or pane had focus before.
  - `esc`: Cancel creating a new item and focus on whatever window or pane had focus before.
  - `<any_other_single_key>`: Users can add custom hotkeys for creating new items in the config file, and these hotkeys should also be shown. Selecting them will follow the same logic as creating new terminal panes, but after the pane is created in that new terminal pane we will run the command the user configured for this hotkey. If the user presses a key that is not configured for creating a new item, then we should show a hint message and stay in create mode.
- `e`: Start editing the context clipboard for the active window. This should open a text editor in the terminal view to the clipboard file for the active window (If it does not exist yet it should be created). When the user exits the editor they should be returned to whatever window and or pane had focus before.
- `y`: If the active window has a context clipboard, copy its contents to the system clipboard and show a hint message. If the active window does not have a context clipboard, copy nothing and show a hint message.
  - `esc`: Cancel editing the context clipboard and return to whatever window and pane had focus before without making any changes to the context clipboard.
- `q`: When in window focus mode, this should prompt the user to confirm detaching the entire Sidebar TUI interface. The hint banner's background should become red to indicate that the user is in detach mode.
  - `y`: Detach the Sidebar TUI.
  - `n`: Cancel detaching and return to whatever window or pane had focus before.

### Terminal View

The terminal view should take up the rest of the space to the right of the sidebar. It should show the terminal pane of the currently active window and pane. When control is swapped to the terminal view, all input should be sent to the terminal pane. When control is swapped to the sidebar, no input should be sent to the terminal pane.

Terminal panes should be managed such that they stay running when the Sidebar client is detached, but also that they pick up where they left off if Sidebar TUI is detached and reopened.

Any terminal that had a full TUI open in it should be resumed to that state when Sidebar TUI is detached and reopened. For example if the user had a text editor open in a terminal pane, then when they detach Sidebar TUI and reopened it, that terminal pane should still have that editor open in it.

### Hint Banner

The hint banner should show the available hotkeys based on the current context. It should span the entire bottom of the pane and wrap to multiple lines if there are too many hotkeys to fit on one line. The background of the hint banner should be the primary color (of the current session), and the text should be black.

**Visibility rules:**

- In the normal Sidebar View + Terminal View mode: The hint banner appears when focus is on the sidebar and disappears when focus is on the terminal view.
- In the Sessions View: The hint banner is always visible, showing session-related hotkeys. If there is a "source" session (the user opened Sessions View from an active session), use that session's primary color. Otherwise, use a neutral default color (e.g., a muted gray-blue).

Sometimes we want to show a hint message. This is just a message that appears in the hint banner for a few seconds.

### Sessions View

The Sessions View is a full-screen view that takes up the entire pane except for the hint banner at the bottom. This view is used to create, select, and manage sessions. The Sessions View replaces both the Sidebar View and Terminal View when active.

#### When the Sessions View Opens

When the Sidebar TUI is started, it should check the current working directory:

- If the working directory is at or inside the base directory of any configured session, that session should be automatically opened and the user goes directly to the normal Sidebar View + Terminal View.
- If the working directory is not inside any session's base directory, the Sessions View should open instead.

#### Visual Layout

The Sessions View should display a list of all configured sessions, centered both horizontally and vertically in the available space. Each session entry should show:

- The session name (highlighted with that session's primary color as the background)
- The base directory path (in a muted color, below the name)
- An indicator if this session matches the current working directory (e.g., a checkmark or "current" label)

The currently focused session should have a visible selection indicator (e.g., a border or arrow). At the bottom of the list, there should be a "+ New Session" option.

#### Hotkeys in Sessions View

- `up arrow`: Move focus to the session above. If at the top, wrap to the bottom.
- `down arrow`: Move focus to the session below. If at the bottom, wrap to the top.
- `enter`: Select the focused session and open it. This switches to the normal Sidebar View + Terminal View for that session.
- `n`: Start creating a new session (see New Session Flow below).
- `r`: Rename the focused session. The name becomes editable with a blinking cursor. Same character restrictions as window/pane names.
  - `enter`: Save the new name.
  - `esc`: Cancel and revert to the original name.
- `d`: Mark the focused session for deletion. The entry's background becomes red. The hint banner's background should become red to indicate delete mode.
  - `y`: Delete the session. This removes the session configuration but does NOT delete the base directory or any files within it. If the deleted session was the only one, stay in Sessions View. Otherwise, focus moves to the next session.
  - `n`: Cancel deletion.
- `c`: Edit the focused session's primary color. A color picker or color code input should appear.
  - `enter`: Save the new color.
  - `esc`: Cancel and revert to the original color.
- `q`: Detach the Sidebar TUI entirely.
  - `y`: Confirm detach.
  - `n`: Cancel.

#### New Session Flow

When the user presses `n` to create a new session:

1. First, prompt for the session name. An input field appears with a blinking cursor. Same character restrictions as window/pane names.
   - `enter`: Proceed to base directory selection.
   - `esc`: Cancel session creation.
2. Then, prompt for the base directory. The input should default to the current working directory. The user can type a path, and tab completion would be nice to have.
   - `enter`: Create the session with the given name and base directory. A random, vibrant primary color is auto-assigned (unique from other sessions if possible). The new session is automatically selected and opened.
   - `esc`: Cancel session creation.

#### Accessing Sessions View from Normal Mode

When in the normal Sidebar View (window or pane focus mode):

- `w`: Open the Sessions View. The current session should be pre-focused in the list.

This allows users to switch sessions without detaching and restarting the TUI.

#### Session Switching Behavior

When the user selects a different session from the Sessions View:

- The current session's state (active window, active pane, focus mode) is saved.
- The new session is loaded with its previously saved state.
- If the new session has no windows yet, focus should be on the sidebar in window focus mode so the user can create a new window.
- If the new session has windows but no active pane was saved, the most recently used window and its most recently used pane become active.

#### Returning from Sessions View

- `esc`: If the user entered the Sessions View from an active session (via `w` or `mod + w`), pressing `esc` returns to that session without making changes. The session they came from should be restored exactly as it was.
- If the user is in Sessions View because no session matched the working directory on startup, `esc` does nothing (they must select or create a session).

#### Edge Cases

- **No sessions exist:** On first run when no sessions are configured, the Sessions View opens with only the "+ New Session" option visible. A helpful message should indicate this is the first time and prompt the user to create a session.
- **Session base directory no longer exists:** If a session's base directory has been deleted or moved, display a warning indicator next to that session in the Sessions View. When selected, prompt the user to update the base directory or delete the session.
- **Multiple sessions match working directory:** If the working directory is inside multiple session base directories (nested sessions), select the most specific one (deepest path match).

### Other Sidebar TUI CLI Commands

- `sb trace`: Running `sb trace` should print the name of terminal pane, window, and session for the current terminal pane that is running in Sidebar TUI. If this command is not called from a Sidebar TUI terminal pane it should print a helpful error message explaining how to use this command.

### Settings & Context

#### Sessions

A session is a named container that groups related windows and panes together. Each session is tied to a base directory on the filesystem. Sessions are the top-level organizational unit in Sidebar TUI.

**Session Definition:**

- **Name:** A human-readable identifier for the session.
- **Base Directory:** The root directory associated with this session. New terminal panes in this session start in this directory by default.
- **Primary Color:** A vibrant color used to visually distinguish this session (in the session header, Sessions View entries, etc.).
- **Windows:** Each session has its own independent set of windows and panes.

**Session Registry:**
The list of all sessions and their configurations is stored in `~/.sidebar_tui/sessions.json`. This file contains:

- An array of session definitions (name, base directory, primary color)
- The ID/name of the last active session

**Session State:**
Each session's runtime state (windows, panes, pinned items, last used timestamps, active window/pane) is stored separately in `~/.sidebar_tui/sessions/<session_id>/state.json`. This keeps session data isolated and prevents the main config from becoming bloated.

#### Window and Pane Persistence

We want to remember the windows and panes that the user had open last time, when they were used last, and which ones are pinned, so that when they open that session again, they can pick up where they left off. We also want to remember which windows are pinned, and the order of the windows. This is stored per-session in the session state file.

Window context clipboards are stored in `~/.sidebar_tui/sessions/<session_id>/clipboards/<window_id>.txt`.

#### Session Customizations

Some customizations are specific to the user globally and some are specific to each session.

- **Base Directory:** The base directory is defined per session. By default when terminal panes are created they should be started in their session's base directory.
- **Primary Color:** The primary color is defined per session. When a new session is created we should default the primary color to a random, vibrant color that, if possible, is unique from the primary colors of other sessions. The color can be edited via the Sessions View.
- **Pane Templates:** The user can configure custom pane templates that show up as options when creating a new pane. A pane template is just a key character (must be a single keyboard character) and a command. When the user selects a pane template, it creates a new terminal pane and runs the configured command in it to set it up. These can be customized per session or globally. If a pane template has the same key character in both the global config and the session config, the session config should take precedence.

Session customizations can be configured either in the global Sidebar TUI config or in the base directory of the session. Session customizations should be resolved in this order:

- Session customizations `.sidebar_tui_config.json` in the base directory of the session takes highest precedence.
- Session customizations `.sidebar_tui/sidebar_tui_config.json` in the base directory of the session takes the next level of precedence.
- Session-specific settings in `~/.sidebar_tui/sessions/<session_id>/config.json` take the next level.
- Global defaults in `~/.sidebar_tui/config.json` take the lowest level of precedence.

#### Auto-Discovery of Sessions

When a session is created with a base directory that contains a `.sidebar_tui_config.json` or `.sidebar_tui/sidebar_tui_config.json` file, those settings are automatically merged with the session configuration. This allows project-specific defaults to be version-controlled while keeping runtime state (windows, panes) private to the user's machine.
