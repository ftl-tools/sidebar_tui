# Workspaces

Workspaces group terminal sessions so you can organize by project, context, or anything else.

## Key properties

- Every session belongs to exactly one workspace.
- Sessions in other workspaces keep running when you switch — they're just hidden.
- Each workspace saves its full view state: which session was last selected, which pane was focused, sidebar scroll position, and terminal scroll history. This state is restored when you switch back.
- Workspaces persist across restarts.
- There must always be at least one workspace. If deleting one would leave none, a new **Default** workspace is auto-created.
- New sessions are always created in the currently active workspace.

## Opening the overlay

Press <kbd>Ctrl+W</kbd> from any pane (or <kbd>w</kbd> from the sidebar) to open the workspace overlay.

## Switching workspaces

In the overlay, navigate with <kbd>↑</kbd>/<kbd>↓</kbd> and press <kbd>Enter</kbd> to switch. The overlay closes and the selected workspace's last saved state is restored.

The currently active workspace is marked with a `*`.

## Creating a workspace

In the overlay, press <kbd>n</kbd>, type a name, and press <kbd>Enter</kbd>.

## Renaming a workspace

In the overlay, select the workspace and press <kbd>r</kbd>. Edit the name and press <kbd>Enter</kbd>.

## Deleting a workspace

In the overlay, select the workspace and press <kbd>d</kbd>. A confirmation prompt appears (red background). Press <kbd>y</kbd> to confirm. All sessions in the deleted workspace are permanently removed.

If you delete the active workspace, sidebar-tui switches to the first remaining workspace (or creates a new Default if none remain).

## Moving a session to another workspace

In the sidebar, select the session you want to move and press <kbd>m</kbd>. The workspace overlay opens in move mode. Select the destination workspace and press <kbd>Enter</kbd>. The session is moved; the overlay closes and focus returns to the sidebar.
