# Sidebar TUI

I want a simple TUI for managing terminal sessions in a sidebar in a way that works with my workflow. I want to have sessions grouped into threads on the side bar, and to be able to easily create new ones and switch between them.

## Spec

Sidebar TUI should be a TUI that opens when I run `sb` in the terminal. The TUI should have a Sidebar View on the left, a Terminal View on the right, and a contextual hints banner across the bottom.

### Workspace Header

At the very top of the sidebar view, the current workspace name should be displayed prominently. The background of this header should be the workspace's primary color, making it immediately clear which workspace is active. The text should be black (or white if the primary color is too dark) for contrast. This header should span the full width of the sidebar.

### Sidebar View

The sidebar view should be a fixed width. It should contain a list of thread names and have a slightly lighter background than the terminal view. The session names of the active thread should be listed under it. None of the other sessions should be visible. The background of the active thread name and all its session names should be highlighted a little lighter. The name of the active session should be the primary color. This way users can easily see which thread and session they are in. No names should be truncated, just wrapped to the next line if they are too long.

Pinned threads are always visible at the top of the sidebar, the rest of the threads are listed below in order of most recently used. A primary colored separator should divide the pinned threads from the unpinned threads. Pinned sessions are always visible at the top of their thread's session list, the rest of the sessions are listed below in order of most recently used. A text colored separator should divide the pinned sessions from the unpinned sessions within each thread.

#### Hotkeys

Our primary mod key is `ctrl` on Mac and `alt` on linux and windows. We refer to this key below as `mod`.

When in session focus mode all input is sent to the terminal view, but the following hotkeys will swap control to the sidebar:

- `mod + b` or `mod + s`: Focus on the active session of the active thread in the sidebar.
- `mod + t`: Focus on the active thread in the sidebar.
- `mod + w`: Open the Workspaces View to switch workspaces or manage them. The current workspace will be pre-focused.
- `mod + n`: Same as the `n` option when focused on the sidebar, but lets the user jump straight to creating a new item. If they cancel, then focus should be returned to the terminal view, not the sidebar.

When in session focus mode:

- `up arrow`: Swap the above session to be the active session. If this is the top session in the thread don't do anything.
- `down arrow`: Swap the below session to be the active session. If this is the bottom session in the thread don't do anything.
- `left arrow`: Focus on the active thread.
- `right arrow` or `enter`: Swap control back to the terminal view of the currently selected session.
- `esc`: Swap back to the original active session and swap control back to the terminal view.

When in thread focus mode:

- `up arrow`: Swap the above thread to be the active thread. If this is the top thread don't do anything. When swapping the active thread, the active session should be set to whatever session was active the last time this thread was active. If this thread has never been active, the active session should be set to the top session in the thread.
- `down arrow`: Swap the below thread to be the active thread. If this is the bottom thread don't do anything. When swapping the active thread, the active session should be set to whatever session was active the last time this thread was active. If this thread has never been active, the active session should be set to the top session in the thread.
- `right arrow` or `enter`: Focus on the active session of the active thread.

When in either thread or session focus mode:

- `w`: Open the Workspaces View to switch to a different workspace or manage workspaces. The current workspace will be pre-focused in the list.
- `esc`: Swap back to the originally active thread and session from before the focus was swapped to the sidebar, and swap control back to the terminal view.
- `b`: Swap between thread focus mode and session focus mode. (This feature might get axed.)
- `p`: Pin the currently focused thread or session. (Stay focused on the item that was just pinned, and show a hint message that the item is now pinned.)
- `u`: Unpin the currently focused thread or session. (Stay focused on the item that was just unpinned, and show a hint message that the item is now unpinned.)
- `d`: Highlight the background of the current sesson or thread (including all its sessions) red to indicate that it is being marked for deletion. The hint banner's background should become red to indicate that the user is in delete mode.
  - `y`: Delete the currently highlighted thread or session. If a thread is deleted, all its sessions are also deleted. If the active thread or session is deleted, the active thread should become the most recently used thread that is still open, and the active session should become the most recently used session in that thread that is still open.
  - `n`: Cancel deletion and remove the red highlight.
- `r`: Rename the currently focused thread or session. The name should become editable with a blinking cursor. The user can type the new name, and upper and lower case letters, numbers, spaces, hyphens, and underscores should be allowed, all other characters should be ignored.
  - `enter`: Save the new name and exit rename mode.
  - `esc`: Cancel renaming and revert to the original name, then exit rename mode.
- `n`: Give users the option of creating a new item.
  - `t`: Start drafting a new thread. It should add a new entry for a thread name at the top of the unpinned thread list with a blinking cursor, indicating that the user is drafting a new thread name. They can type the name of the new thread. Upper and lower case letters, numbers, spaces, hyphens, and underscores should be allowed, all other characters should be ignored.
    - `enter`: Create the new thread. There will be no active session. The user can use the hotkeys to create the kind of session they want.
    - `esc`: Cancel drafting the new thread and focus on whatever thread or session had focus before.
  - `s`: Start drafting a new terminal session in the active thread. It should add a new entry for a session name at the top of the thread's list of sessions with a blinking cursor, indicating that the user is drafting a new session name. They can type the name of the new session. Upper and lower case letters, numbers, spaces, hyphens, and underscores should be allowed, all other characters should be ignored.
    - `enter`: Create the new session and swap control to it in the terminal view.
    - `esc`: Cancel drafting the new session and focus on whatever thread or session had focus before.
  - `esc`: Cancel creating a new item and focus on whatever thread or session had focus before.
  - `<any_other_single_key>`: Users can add custom hotkeys for creating new items in the config file, and these hotkeys should also be shown. Selecting them will follow the same logic as creating new terminal sessions, but after the session is created in that new terminal session we will run the command the user configured for this hotkey. If the user presses a key that is not configured for creating a new item, then we should show a hint message and stay in create mode.
- `e`: Start editing the context clipboard for the active thread. This should open a text editor in the terminal view to the clipboard file for the active thread (If it does not exist yet it should be created). When the user exits the editor they should be returned to whatever thread and or session had focus before.
- `y`: If the active thread has a context clipboard, copy its contents to the system clipboard and show a hint message. If the active thread does not have a context clipboard, copy nothing and show a hint message.
  - `esc`: Cancel editing the context clipboard and return to whatever thread and session had focus before without making any changes to the context clipboard.
- `q`: When in thread focus mode, this should prompt the user to confirm quitting the entire Sidebar TUI interface. The hint banner's background should become red to indicate that the user is in quit mode.
  - `y`: Quit the Sidebar TUI.
  - `n`: Cancel quitting and return to whatever thread or session had focus before.

### Terminal View

The terminal view should take up the rest of the space to the right of the sidebar. It should show the terminal session of the currently active thread and session. When control is swapped to the terminal view, all input should be sent to the terminal session. When control is swapped to the sidebar, no input should be sent to the terminal session.

Terminal sessions should be managed such that they do not stay running when Sidebar TUI is quit, but also that they pick up where they left off if Sidebar TUI is quit and reopened.

Any terminal that had a full TUI open in it should be resumed to that state when Sidebar TUI is quit and reopened. For example if the user had a text editor open in a terminal session, then when they quit Sidebar TUI and reopened it, that terminal session should still have that editor open in it.

### Hint Banner

The hint banner should show the available hotkeys based on the current context. It should span the entire bottom of the window and wrap to multiple lines if there are too many hotkeys to fit on one line. The background of the hint banner should be the primary color (of the current workspace), and the text should be black.

**Visibility rules:**

- In the normal Sidebar View + Terminal View mode: The hint banner appears when focus is on the sidebar and disappears when focus is on the terminal view.
- In the Workspaces View: The hint banner is always visible, showing workspace-related hotkeys. If there is a "source" workspace (the user opened Workspaces View from an active workspace), use that workspace's primary color. Otherwise, use a neutral default color (e.g., a muted gray-blue).

Sometimes we want to show a hint message. This is just a message that appears in the hint banner for a few seconds.

### Workspaces View

The Workspaces View is a full-screen view that takes up the entire window except for the hint banner at the bottom. This view is used to create, select, and manage workspaces. The Workspaces View replaces both the Sidebar View and Terminal View when active.

#### When the Workspaces View Opens

When the Sidebar TUI is started, it should check the current working directory:

- If the working directory is at or inside the base directory of any configured workspace, that workspace should be automatically opened and the user goes directly to the normal Sidebar View + Terminal View.
- If the working directory is not inside any workspace's base directory, the Workspaces View should open instead.

#### Visual Layout

The Workspaces View should display a list of all configured workspaces, centered both horizontally and vertically in the available space. Each workspace entry should show:

- The workspace name (highlighted with that workspace's primary color as the background)
- The base directory path (in a muted color, below the name)
- An indicator if this workspace matches the current working directory (e.g., a checkmark or "current" label)

The currently focused workspace should have a visible selection indicator (e.g., a border or arrow). At the bottom of the list, there should be a "+ New Workspace" option.

#### Hotkeys in Workspaces View

- `up arrow`: Move focus to the workspace above. If at the top, wrap to the bottom.
- `down arrow`: Move focus to the workspace below. If at the bottom, wrap to the top.
- `enter`: Select the focused workspace and open it. This switches to the normal Sidebar View + Terminal View for that workspace.
- `n`: Start creating a new workspace (see New Workspace Flow below).
- `r`: Rename the focused workspace. The name becomes editable with a blinking cursor. Same character restrictions as thread/session names.
  - `enter`: Save the new name.
  - `esc`: Cancel and revert to the original name.
- `d`: Mark the focused workspace for deletion. The entry's background becomes red. The hint banner's background should become red to indicate delete mode.
  - `y`: Delete the workspace. This removes the workspace configuration but does NOT delete the base directory or any files within it. If the deleted workspace was the only one, stay in Workspaces View. Otherwise, focus moves to the next workspace.
  - `n`: Cancel deletion.
- `c`: Edit the focused workspace's primary color. A color picker or color code input should appear.
  - `enter`: Save the new color.
  - `esc`: Cancel and revert to the original color.
- `q`: Quit the Sidebar TUI entirely.
  - `y`: Confirm quit.
  - `n`: Cancel.

#### New Workspace Flow

When the user presses `n` to create a new workspace:

1. First, prompt for the workspace name. An input field appears with a blinking cursor. Same character restrictions as thread/session names.
   - `enter`: Proceed to base directory selection.
   - `esc`: Cancel workspace creation.
2. Then, prompt for the base directory. The input should default to the current working directory. The user can type a path, and tab completion would be nice to have.
   - `enter`: Create the workspace with the given name and base directory. A random, vibrant primary color is auto-assigned (unique from other workspaces if possible). The new workspace is automatically selected and opened.
   - `esc`: Cancel workspace creation.

#### Accessing Workspaces View from Normal Mode

When in the normal Sidebar View (thread or session focus mode):

- `w`: Open the Workspaces View. The current workspace should be pre-focused in the list.

This allows users to switch workspaces without quitting and restarting the TUI.

#### Workspace Switching Behavior

When the user selects a different workspace from the Workspaces View:

- The current workspace's state (active thread, active session, focus mode) is saved.
- The new workspace is loaded with its previously saved state.
- If the new workspace has no threads yet, focus should be on the sidebar in thread focus mode so the user can create a new thread.
- If the new workspace has threads but no active session was saved, the most recently used thread and its most recently used session become active.

#### Returning from Workspaces View

- `esc`: If the user entered the Workspaces View from an active workspace (via `w` or `mod + w`), pressing `esc` returns to that workspace without making changes. The workspace they came from should be restored exactly as it was.
- If the user is in Workspaces View because no workspace matched the working directory on startup, `esc` does nothing (they must select or create a workspace).

#### Edge Cases

- **No workspaces exist:** On first run when no workspaces are configured, the Workspaces View opens with only the "+ New Workspace" option visible. A helpful message should indicate this is the first time and prompt the user to create a workspace.
- **Workspace base directory no longer exists:** If a workspace's base directory has been deleted or moved, display a warning indicator next to that workspace in the Workspaces View. When selected, prompt the user to update the base directory or delete the workspace.
- **Multiple workspaces match working directory:** If the working directory is inside multiple workspace base directories (nested workspaces), select the most specific one (deepest path match).

### Other Sidebar TUI CLI Commands

- `sb trace`: Running `sb trace` should print the name of terminal session, thread, and workspace for the current terminal session that is running in Sidebar TUI. If this command is not called from a Sidebar TUI terminal session it should print a helpful error message explaining how to use this command.

### Settings & Context

#### Workspaces

A workspace is a named container that groups related threads and sessions together. Each workspace is tied to a base directory on the filesystem. Workspaces are the top-level organizational unit in Sidebar TUI.

**Workspace Definition:**

- **Name:** A human-readable identifier for the workspace.
- **Base Directory:** The root directory associated with this workspace. New terminal sessions in this workspace start in this directory by default.
- **Primary Color:** A vibrant color used to visually distinguish this workspace (in the workspace header, Workspaces View entries, etc.).
- **Threads:** Each workspace has its own independent set of threads and sessions.

**Workspace Registry:**
The list of all workspaces and their configurations is stored in `~/.sidebar_tui/workspaces.json`. This file contains:

- An array of workspace definitions (name, base directory, primary color)
- The ID/name of the last active workspace

**Workspace State:**
Each workspace's runtime state (threads, sessions, pinned items, last used timestamps, active thread/session) is stored separately in `~/.sidebar_tui/workspaces/<workspace_id>/state.json`. This keeps workspace data isolated and prevents the main config from becoming bloated.

#### Thread and Session Persistence

We want to remember the threads and sessions that the user had open last time, when they were used last, and which ones are pinned, so that when they open that workspace again, they can pick up where they left off. We also want to remember which threads are pinned, and the order of the threads. This is stored per-workspace in the workspace state file.

Thread context clipboards are stored in `~/.sidebar_tui/workspaces/<workspace_id>/clipboards/<thread_id>.txt`.

#### Workspace Customizations

Some customizations are specific to the user globally and some are specific to each workspace.

- **Base Directory:** The base directory is defined per workspace. By default when terminal sessions are created they should be started in their workspace's base directory.
- **Primary Color:** The primary color is defined per workspace. When a new workspace is created we should default the primary color to a random, vibrant color that, if possible, is unique from the primary colors of other workspaces. The color can be edited via the Workspaces View.
- **Session Templates:** The user can configure custom session templates that show up as options when creating a new session. A session template is just a key character (must be a single keyboard character) and a command. When the user selects a session template, it creates a new terminal session and runs the configured command in it to set it up. These can be customized per workspace or globally. If a session template has the same key character in both the global config and the workspace config, the workspace config should take precedence.

Workspace customizations can be configured either in the global Sidebar TUI config or in the base directory of the workspace. Workspace customizations should be resolved in this order:

- Workspace customizations `.sidebar_tui_config.json` in the base directory of the workspace takes highest precedence.
- Workspace customizations `.sidebar_tui/sidebar_tui_config.json` in the base directory of the workspace takes the next level of precedence.
- Workspace-specific settings in `~/.sidebar_tui/workspaces/<workspace_id>/config.json` take the next level.
- Global defaults in `~/.sidebar_tui/config.json` take the lowest level of precedence.

#### Auto-Discovery of Workspaces

When a workspace is created with a base directory that contains a `.sidebar_tui_config.json` or `.sidebar_tui/sidebar_tui_config.json` file, those settings are automatically merged with the workspace configuration. This allows project-specific defaults to be version-controlled while keeping runtime state (threads, sessions) private to the user's machine.
