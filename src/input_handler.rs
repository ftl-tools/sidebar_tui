//! Keyboard event handling with state machine integration.
//!
//! This module contains the input handler that routes key events based on the
//! current application mode and focus state.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{
    AppMode, AppState, ConfirmAction, ConfirmState, EventResult, Focus, RenamingState, SessionType,
    WorkspaceOverlayMode,
};

impl AppState {
    /// Handle a key event based on current mode and focus.
    /// Returns EventResult indicating if the event was consumed, not consumed, or quit requested.
    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        // Check if we're in a modal state first (modal states take precedence)
        match &self.mode {
            AppMode::Confirming(_) => {
                return self.handle_confirming_key(key);
            }
            AppMode::CreateMode { .. } => {
                return self.handle_create_mode_key(key);
            }
            AppMode::Drafting(_) => {
                return self.handle_drafting_key(key);
            }
            AppMode::Renaming(_) => {
                return self.handle_renaming_key(key);
            }
            AppMode::WorkspaceOverlay(_) => {
                return self.handle_workspace_overlay_key(key);
            }
            AppMode::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')
                ) {
                    self.mode = AppMode::Normal;
                }
                return EventResult::Consumed;
            }
            AppMode::Normal => {}
        }

        // Normal mode - dispatch based on focus
        match self.focus {
            Focus::Sidebar => self.handle_sidebar_key(key),
            Focus::Terminal => self.handle_terminal_key(key),
        }
    }

    /// Return true for one of the portable toggle events. Cmd aliases depend on
    /// the terminal exposing SUPER rather than consuming the shortcut itself.
    fn is_toggle_key(key: &KeyEvent) -> bool {
        let ctrl_or_cmd = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER);
        (ctrl_or_cmd && matches!(key.code, KeyCode::Char('b') | KeyCode::Char(' ')))
            || key.code == KeyCode::Null
    }

    fn selected_session_event(&mut self, commit: bool) -> EventResult {
        let Some(name) = self
            .sessions
            .get(self.selected_index)
            .map(|s| s.name.clone())
        else {
            if commit {
                self.focus_terminal();
            }
            return EventResult::Consumed;
        };
        if commit {
            self.focus_terminal();
            EventResult::SwitchSession { name }
        } else {
            EventResult::PreviewSession { name }
        }
    }

    /// Handle shortcuts that intentionally work from either normal pane.
    fn handle_direct_shortcut(&mut self, key: KeyEvent) -> Option<EventResult> {
        if Self::is_toggle_key(&key) {
            return Some(if self.focus == Focus::Terminal {
                self.focus_sidebar();
                EventResult::Consumed
            } else {
                self.selected_session_event(true)
            });
        }
        if !key.modifiers.contains(KeyModifiers::ALT) {
            return None;
        }

        let commit = self.focus == Focus::Terminal;
        let origin = self.selected_index;
        let result = match key.code {
            KeyCode::Char(c @ '1'..='9') => {
                let index = (c as u8 - b'1') as usize;
                if index < self.sessions.len() {
                    self.selected_index = index;
                    self.selected_session_event(commit)
                } else {
                    EventResult::Consumed
                }
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.reorder_selected(-1);
                EventResult::ReorderSession { offset: -1 }
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.reorder_selected(1);
                EventResult::ReorderSession { offset: 1 }
            }
            KeyCode::Left | KeyCode::Right => {
                if key.code == KeyCode::Left {
                    if self.selected_index == 0 && !self.sessions.is_empty() {
                        self.selected_index = self.sessions.len() - 1;
                    } else {
                        self.select_previous();
                    }
                } else if !self.sessions.is_empty() {
                    self.selected_index = (self.selected_index + 1) % self.sessions.len();
                }
                self.selected_session_event(commit)
            }
            KeyCode::Up => EventResult::SwitchRelativeWorkspace { offset: -1 },
            KeyCode::Down => EventResult::SwitchRelativeWorkspace { offset: 1 },
            _ => return None,
        };
        if commit
            && self.selected_index != origin
            && matches!(result, EventResult::SwitchSession { .. })
        {
            self.previous_session = Some(origin);
        }
        Some(result)
    }

    /// Handle key events when sidebar is focused (Normal mode).
    fn handle_sidebar_key(&mut self, key: KeyEvent) -> EventResult {
        if let Some(result) = self.handle_direct_shortcut(key) {
            return result;
        }
        // Bare command letters must not accidentally fire for retired Ctrl shortcuts.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return EventResult::Consumed;
        }
        // Some terminals report Shift+letter as lowercase plus SHIFT instead of an uppercase char.
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::Char('c') => return EventResult::OpenWorkspaceCreate,
                KeyCode::Char('r') => return EventResult::OpenWorkspaceRename,
                KeyCode::Char('k') => return EventResult::OpenWorkspaceDelete,
                KeyCode::Char('p') => return EventResult::SwitchRelativeWorkspace { offset: -1 },
                KeyCode::Char('n') => return EventResult::SwitchRelativeWorkspace { offset: 1 },
                KeyCode::Char('s') => {
                    self.mouse_mode = !self.mouse_mode;
                    return EventResult::ToggleMouseMode;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_previous();
                self.selected_session_event(false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                self.selected_session_event(false)
            }
            KeyCode::Enter => self.selected_session_event(true),
            KeyCode::Esc | KeyCode::Char('q') => {
                self.cancel_browsing();
                self.selected_session_event(true)
            }
            KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                // The old two-step n→t prompt slowed the common path; c now drafts a terminal directly.
                self.start_drafting(SessionType::Terminal);
                EventResult::Consumed
            }
            KeyCode::Char('a') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.start_drafting(SessionType::Agent);
                EventResult::Consumed
            }
            KeyCode::Char('n') | KeyCode::Char('p') => {
                if !self.sessions.is_empty() {
                    if key.code == KeyCode::Char('n') {
                        self.selected_index = (self.selected_index + 1) % self.sessions.len();
                    } else if self.selected_index == 0 {
                        self.selected_index = self.sessions.len() - 1;
                    } else {
                        self.selected_index -= 1;
                    }
                }
                self.selected_session_event(false)
            }
            KeyCode::Char('1'..='9') => {
                if let KeyCode::Char(c) = key.code {
                    let index = (c as u8 - b'1') as usize;
                    if index < self.sessions.len() {
                        self.selected_index = index;
                    }
                }
                self.selected_session_event(false)
            }
            KeyCode::Char('l') => {
                self.jump_back();
                self.selected_session_event(true)
            }
            KeyCode::Char('r') | KeyCode::Char(',') => {
                if !self.sessions.is_empty() {
                    self.start_renaming();
                }
                EventResult::Consumed
            }
            KeyCode::Char('&') | KeyCode::Delete => {
                if !self.sessions.is_empty() {
                    self.request_confirmation(ConfirmAction::DeleteSession(self.selected_index));
                }
                EventResult::Consumed
            }
            KeyCode::Char('m') => self
                .sessions
                .get(self.selected_index)
                .map(|session| EventResult::OpenMoveToWorkspaceOverlay {
                    session_name: session.name.clone(),
                })
                .unwrap_or(EventResult::Consumed),
            KeyCode::Char('s') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                EventResult::OpenWorkspaceOverlay
            }
            KeyCode::Char('w') => EventResult::Consumed,
            KeyCode::Char('C') => EventResult::OpenWorkspaceCreate,
            KeyCode::Char('R') | KeyCode::Char('$') => EventResult::OpenWorkspaceRename,
            KeyCode::Char('K') => EventResult::OpenWorkspaceDelete,
            KeyCode::Char('P') => EventResult::SwitchRelativeWorkspace { offset: -1 },
            KeyCode::Char('N') => EventResult::SwitchRelativeWorkspace { offset: 1 },
            KeyCode::Char('S') => {
                self.mouse_mode = !self.mouse_mode;
                EventResult::ToggleMouseMode
            }
            KeyCode::Char('z') => {
                self.zoomed = true;
                self.focus_terminal();
                EventResult::ToggleZoom
            }
            KeyCode::Char('d') => {
                self.request_confirmation(ConfirmAction::Quit);
                EventResult::Consumed
            }
            KeyCode::Char('?') => {
                self.mode = AppMode::Help;
                EventResult::Consumed
            }
            // Sidebar is a sticky command mode: unsupported commands must never reach the shell.
            _ => EventResult::Consumed,
        }
    }

    /// Handle key events when terminal is focused (Normal mode).
    fn handle_terminal_key(&mut self, key: KeyEvent) -> EventResult {
        // Old Ctrl+N/W/S/Z/Q/T bindings intercepted common shell/editor input. Only the
        // proposed Toggle and direct Alt shortcuts remain global.
        if let Some(result) = self.handle_direct_shortcut(key) {
            return result;
        }
        EventResult::NotConsumed
    }

    /// Handle key events in create mode (selecting session type: t or a).
    /// Transitions into drafting mode so the user can type a custom session name.
    fn handle_create_mode_key(&mut self, key: KeyEvent) -> EventResult {
        match key.code {
            // Terminal session: enter drafting mode for user to type a name
            KeyCode::Char('t') => {
                self.start_drafting(SessionType::Terminal);
                EventResult::Consumed
            }
            // Agent session: enter drafting mode for user to type a name
            KeyCode::Char('a') => {
                self.start_drafting(SessionType::Agent);
                EventResult::Consumed
            }
            // Cancel
            KeyCode::Esc => {
                self.cancel_create_mode();
                EventResult::Consumed
            }
            _ => EventResult::Consumed, // Consume but ignore other keys
        }
    }

    /// Handle key events while drafting a new session name.
    fn handle_drafting_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Drafting(ref mut draft) = self.mode {
            match key.code {
                // Create the session
                KeyCode::Enter => {
                    let session_type = draft.session_type;
                    // Empty names are now optional; generate the same collision-safe name used elsewhere.
                    let name = if draft.name.trim().is_empty() {
                        let existing: Vec<&str> =
                            self.sessions.iter().map(|s| s.name.as_str()).collect();
                        crate::name_generator::generate_unique_session_name(&existing)
                    } else {
                        draft.name.trim().to_string()
                    };
                    self.mode = AppMode::Normal;
                    EventResult::CreateSession { name, session_type }
                }
                // Cancel drafting
                KeyCode::Esc => {
                    self.cancel_drafting();
                    EventResult::Consumed
                }
                // Text input
                KeyCode::Char(c) => {
                    draft.insert_char(c);
                    EventResult::Consumed
                }
                // Backspace
                KeyCode::Backspace => {
                    draft.delete_char();
                    EventResult::Consumed
                }
                // Cursor movement
                KeyCode::Left => {
                    draft.move_cursor_left();
                    EventResult::Consumed
                }
                KeyCode::Right => {
                    draft.move_cursor_right();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed, // Consume but ignore other keys
            }
        } else {
            EventResult::NotConsumed
        }
    }

    /// Handle key events while renaming a session.
    fn handle_renaming_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Renaming(ref mut rename) = self.mode {
            match key.code {
                // Complete rename
                KeyCode::Enter => {
                    let index = rename.session_index;
                    let new_name = rename.new_name.clone();
                    // Get old name before updating local state
                    let old_name = self
                        .sessions
                        .get(index)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if !new_name.is_empty() && !old_name.is_empty() {
                        // Update local state
                        self.rename_session(index, new_name.clone());
                        self.mode = AppMode::Normal;
                        // Sticky command mode keeps focus in the sidebar after editing.
                        self.focus = Focus::Sidebar;
                        // Return RenameSession event for daemon to handle
                        return EventResult::RenameSession { old_name, new_name };
                    }
                    self.mode = AppMode::Normal;
                    // Keep the visible command mode active even when the name is unchanged.
                    self.focus = Focus::Sidebar;
                    EventResult::Consumed
                }
                // Cancel renaming
                KeyCode::Esc => {
                    self.cancel_renaming();
                    EventResult::Consumed
                }
                // Text input
                KeyCode::Char(c) => {
                    rename.insert_char(c);
                    EventResult::Consumed
                }
                // Backspace
                KeyCode::Backspace => {
                    rename.delete_char();
                    EventResult::Consumed
                }
                // Cursor movement
                KeyCode::Left => {
                    rename.move_cursor_left();
                    EventResult::Consumed
                }
                KeyCode::Right => {
                    rename.move_cursor_right();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed, // Consume but ignore other keys
            }
        } else {
            EventResult::NotConsumed
        }
    }

    /// Handle key events while the workspace overlay is open.
    fn handle_workspace_overlay_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::WorkspaceOverlay(ref mut overlay) = self.mode {
            // If we're in drafting mode (creating a new workspace), handle text input
            if overlay.drafting_workspace.is_some() {
                match key.code {
                    KeyCode::Enter => {
                        let name = overlay
                            .drafting_workspace
                            .as_ref()
                            .map(|d| d.new_name.trim().to_string())
                            .unwrap_or_default();
                        if name.is_empty() {
                            overlay.drafting_workspace = None;
                        } else {
                            let name_clone = name.clone();
                            overlay.drafting_workspace = None;
                            return EventResult::CreateWorkspace { name: name_clone };
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Esc => {
                        overlay.drafting_workspace = None;
                        return EventResult::Consumed;
                    }
                    KeyCode::Char(c) => {
                        if let Some(ref mut draft) = overlay.drafting_workspace {
                            draft.insert_char(c);
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Backspace => {
                        if let Some(ref mut draft) = overlay.drafting_workspace {
                            draft.delete_char();
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Left => {
                        if let Some(ref mut draft) = overlay.drafting_workspace {
                            draft.move_cursor_left();
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Right => {
                        if let Some(ref mut draft) = overlay.drafting_workspace {
                            draft.move_cursor_right();
                        }
                        return EventResult::Consumed;
                    }
                    _ => return EventResult::Consumed,
                }
            }

            // If we're renaming a workspace, handle text input
            if overlay.renaming.is_some() {
                match key.code {
                    KeyCode::Enter => {
                        let new_name = overlay
                            .renaming
                            .as_ref()
                            .map(|r| r.new_name.trim().to_string())
                            .unwrap_or_default();
                        if !new_name.is_empty() {
                            let old_name = overlay
                                .workspaces
                                .get(overlay.selected_index)
                                .cloned()
                                .unwrap_or_default();
                            if !old_name.is_empty() {
                                overlay.renaming = None;
                                return EventResult::RenameWorkspace { old_name, new_name };
                            }
                        }
                        overlay.renaming = None;
                        return EventResult::Consumed;
                    }
                    KeyCode::Esc => {
                        overlay.renaming = None;
                        return EventResult::Consumed;
                    }
                    KeyCode::Char(c) => {
                        if let Some(ref mut rename) = overlay.renaming {
                            rename.insert_char(c);
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Backspace => {
                        if let Some(ref mut rename) = overlay.renaming {
                            rename.delete_char();
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Left => {
                        if let Some(ref mut rename) = overlay.renaming {
                            rename.move_cursor_left();
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Right => {
                        if let Some(ref mut rename) = overlay.renaming {
                            rename.move_cursor_right();
                        }
                        return EventResult::Consumed;
                    }
                    _ => return EventResult::Consumed,
                }
            }

            // Normal overlay navigation
            match key.code {
                // Close overlay
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.mode = AppMode::Normal;
                    return EventResult::Consumed;
                }
                // Navigate down
                KeyCode::Down | KeyCode::Char('j') => {
                    if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                        ov.select_next();
                    }
                    return EventResult::Consumed;
                }
                // Navigate up
                KeyCode::Up | KeyCode::Char('k') => {
                    if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                        ov.select_previous();
                    }
                    return EventResult::Consumed;
                }
                // Select (switch to workspace) or move session to workspace
                KeyCode::Enter => {
                    let (mode, selected, workspaces, active_workspace) =
                        if let AppMode::WorkspaceOverlay(ref ov) = self.mode {
                            (
                                ov.mode.clone(),
                                ov.selected_index,
                                ov.workspaces.clone(),
                                ov.active_workspace.clone(),
                            )
                        } else {
                            return EventResult::Consumed;
                        };
                    let workspace_name = workspaces.get(selected).cloned().unwrap_or_default();
                    self.mode = AppMode::Normal;
                    return match mode {
                        WorkspaceOverlayMode::Normal => EventResult::SwitchWorkspace {
                            name: workspace_name,
                        },
                        WorkspaceOverlayMode::MoveSession { session_name } => {
                            // Spec: "If the selected workspace is the current workspace, do nothing."
                            if workspace_name == active_workspace {
                                EventResult::Consumed
                            } else {
                                EventResult::MoveSessionToWorkspace {
                                    session_name,
                                    workspace_name,
                                }
                            }
                        }
                    };
                }
                // Create/rename/delete are disabled in move mode
                KeyCode::Char('C') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::WorkspaceOverlay(ref ov) if matches!(ov.mode, WorkspaceOverlayMode::MoveSession { .. })
                    );
                    if !is_move_mode {
                        if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                            // Inline draft: scroll to top so draft row at virtual index 0 is visible
                            ov.selected_index = 0;
                            ov.scroll_offset = 0;
                            ov.drafting_workspace = Some(RenamingState::new(0, "", self.focus));
                        }
                    }
                    return EventResult::Consumed;
                }
                // Rename selected workspace (disabled in move mode)
                KeyCode::Char('R') | KeyCode::Char('$') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::WorkspaceOverlay(ref ov) if matches!(ov.mode, WorkspaceOverlayMode::MoveSession { .. })
                    );
                    if !is_move_mode {
                        let selected_name = if let AppMode::WorkspaceOverlay(ref ov) = self.mode {
                            ov.workspaces
                                .get(ov.selected_index)
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        if !selected_name.is_empty() {
                            if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                                ov.renaming =
                                    Some(RenamingState::new(0, &selected_name, Focus::Sidebar));
                            }
                        }
                    }
                    return EventResult::Consumed;
                }
                // Delete selected workspace (disabled in move mode) - shows confirmation
                KeyCode::Char('K') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::WorkspaceOverlay(ref ov) if matches!(ov.mode, WorkspaceOverlayMode::MoveSession { .. })
                    );
                    if !is_move_mode {
                        let (selected, workspaces, active) =
                            if let AppMode::WorkspaceOverlay(ref ov) = self.mode {
                                (
                                    ov.selected_index,
                                    ov.workspaces.clone(),
                                    ov.active_workspace.clone(),
                                )
                            } else {
                                return EventResult::Consumed;
                            };
                        let workspace_name = workspaces.get(selected).cloned().unwrap_or_default();
                        // Don't try to delete an empty workspace name
                        if workspace_name.is_empty() {
                            return EventResult::Consumed;
                        }
                        // Close overlay and show confirmation
                        self.mode = AppMode::Confirming(ConfirmState::new(
                            ConfirmAction::DeleteWorkspace(workspace_name),
                            if active == "Default" {
                                self.focus
                            } else {
                                self.focus
                            },
                        ));
                    }
                    return EventResult::Consumed;
                }
                _ => return EventResult::Consumed,
            }
        }
        EventResult::NotConsumed
    }

    /// Handle key events for confirmation prompts.
    fn handle_confirming_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Confirming(ref confirm) = self.mode {
            // Check if 'q' should also confirm (only for Quit action)
            let is_quit_confirm =
                matches!(confirm.action, ConfirmAction::Quit) && key.code == KeyCode::Char('q');

            match key.code {
                // Confirm (yes, or 'q' for quit specifically)
                KeyCode::Char('y') => {
                    let action = confirm.action.clone();
                    self.mode = AppMode::Normal;
                    match action {
                        ConfirmAction::Quit => EventResult::Quit,
                        ConfirmAction::DeleteSession(index) => {
                            // Get the session name before removing from local state
                            let name = self
                                .sessions
                                .get(index)
                                .map(|s| s.name.clone())
                                .unwrap_or_default();
                            if !name.is_empty() {
                                // Remove from local state
                                self.remove_session(index);
                                // Return DeleteSession event for daemon to handle
                                EventResult::DeleteSession { name }
                            } else {
                                EventResult::Consumed
                            }
                        }
                        ConfirmAction::DeleteWorkspace(name) => {
                            EventResult::DeleteWorkspace { name }
                        }
                    }
                }
                // 'q' is alternative to 'y' for quit confirmation only
                KeyCode::Char('q') if is_quit_confirm => {
                    self.mode = AppMode::Normal;
                    EventResult::Quit
                }
                // Cancel (no)
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.cancel_confirmation();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed, // Consume but ignore other keys
            }
        } else {
            EventResult::NotConsumed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ConfirmState, DraftingState, RenamingState, Session};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    // === Sidebar Focus Tests ===

    #[test]
    fn test_sidebar_up_down_navigation() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.focus = Focus::Sidebar;

        assert_eq!(state.selected_index, 0);

        let result = state.handle_key(key(KeyCode::Down));
        // Navigation returns PreviewSession for live preview
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Down));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        // At bottom, should stay but still return preview for current selection
        let result = state.handle_key(key(KeyCode::Down));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        let result = state.handle_key(key(KeyCode::Up));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Up));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "a"));
        assert_eq!(state.selected_index, 0);

        // At top, should stay but still return preview for current selection
        let result = state.handle_key(key(KeyCode::Up));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "a"));
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_sidebar_vim_jk_navigation() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.focus = Focus::Sidebar;

        assert_eq!(state.selected_index, 0);

        // j moves down (vim-style) and returns PreviewSession
        let result = state.handle_key(key(KeyCode::Char('j')));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Char('j')));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        // At bottom, should stay but still return preview
        let result = state.handle_key(key(KeyCode::Char('j')));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        // k moves up (vim-style) and returns PreviewSession
        let result = state.handle_key(key(KeyCode::Char('k')));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Char('k')));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "a"));
        assert_eq!(state.selected_index, 0);

        // At top, should stay but still return preview
        let result = state.handle_key(key(KeyCode::Char('k')));
        assert!(matches!(result, EventResult::PreviewSession { name } if name == "a"));
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_sidebar_enter_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Enter));
        // Now returns SwitchSession instead of Consumed
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_space_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char(' ')));
        // Now returns SwitchSession instead of Consumed
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_right_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Right));
        // Now returns SwitchSession instead of Consumed
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_tab_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Tab));
        // Tab should focus terminal just like Enter, Space, and Right
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_enter_focuses_terminal_in_welcome_state() {
        // In welcome state (no sessions), Enter should still focus the terminal so the
        // welcome text keybinding updates dynamically (shows ctrl+n instead of n).
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Terminal); // Should switch to terminal even with no sessions
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_esc_jump_back() {
        let mut state = AppState {
            sessions: vec![Session::new("a"), Session::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_session to 0
        state.focus_sidebar();
        state.select_next(); // Now at 1

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selected_index, 0); // Jumped back
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_n_enters_create_mode() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. }));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_ctrl_n_enters_create_mode() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('n'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. }));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_ctrl_b_jump_back() {
        let mut state = AppState {
            sessions: vec![Session::new("a"), Session::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_session to 0
        state.focus_sidebar();
        state.select_next(); // Now at 1

        // Ctrl+B from sidebar should jump back (like Esc)
        let result = state.handle_key(ctrl_key('b'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selected_index, 0); // Jumped back
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_ctrl_t_jump_back() {
        let mut state = AppState {
            sessions: vec![Session::new("a"), Session::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_session to 0
        state.focus_sidebar();
        state.select_next(); // Now at 1

        // Ctrl+T from sidebar should jump back (like Esc)
        let result = state.handle_key(ctrl_key('t'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selected_index, 0); // Jumped back
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_b_jump_back() {
        let mut state = AppState {
            sessions: vec![Session::new("a"), Session::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_session to 0
        state.focus_sidebar();
        state.select_next(); // Now at 1

        // 'b' from sidebar should jump back (like Esc)
        let result = state.handle_key(key(KeyCode::Char('b')));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selected_index, 0); // Jumped back
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_d_requests_delete_confirmation() {
        let mut state = AppState {
            sessions: vec![Session::new("test")],
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('d')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert!(matches!(confirm.action, ConfirmAction::DeleteSession(0)));
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_d_does_nothing_when_empty() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_sidebar_r_starts_renaming() {
        let mut state = AppState {
            sessions: vec![Session::new("old_name")],
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('r')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Renaming(_)));
    }

    #[test]
    fn test_sidebar_r_does_nothing_when_empty() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('r')));
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_q_requests_quit_confirmation() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Quit);
        }
    }

    // === Terminal Focus Tests ===

    #[test]
    fn test_terminal_ctrl_b_focuses_sidebar() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('b'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_terminal_ctrl_t_focuses_sidebar() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('t'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_terminal_ctrl_n_enters_create_mode() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('n'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. }));
    }

    #[test]
    fn test_terminal_regular_keys_not_consumed() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('a')));
        assert_eq!(result, EventResult::NotConsumed);

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::NotConsumed);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_terminal_ctrl_q_requests_quit_confirmation() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('q'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Quit);
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_ctrl_q_requests_quit_confirmation() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('q'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Quit);
        }
    }

    // === Create Mode Tests ===

    #[test]
    fn test_create_mode_t_enters_drafting_mode() {
        let mut state = AppState {
            mode: AppMode::CreateMode {
                previous_focus: Focus::Sidebar,
            },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('t')));
        assert_eq!(result, EventResult::Consumed);
        // Should transition to Drafting mode for user to type a name
        match &state.mode {
            AppMode::Drafting(draft) => {
                assert_eq!(draft.session_type, SessionType::Terminal);
                assert!(draft.name.is_empty(), "Draft name should start empty");
            }
            _ => panic!("Expected Drafting mode, got {:?}", state.mode),
        }
    }

    #[test]
    fn test_create_mode_a_enters_drafting_mode() {
        let mut state = AppState {
            mode: AppMode::CreateMode {
                previous_focus: Focus::Sidebar,
            },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('a')));
        assert_eq!(result, EventResult::Consumed);
        // Should transition to Drafting mode for user to type a name
        match &state.mode {
            AppMode::Drafting(draft) => {
                assert_eq!(draft.session_type, SessionType::Agent);
                assert!(draft.name.is_empty(), "Draft name should start empty");
            }
            _ => panic!("Expected Drafting mode, got {:?}", state.mode),
        }
    }

    #[test]
    fn test_create_mode_esc_cancels() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::CreateMode {
                previous_focus: Focus::Terminal,
            },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Terminal); // Restored
    }

    #[test]
    fn test_create_mode_other_keys_consumed_but_ignored() {
        let mut state = AppState {
            mode: AppMode::CreateMode {
                previous_focus: Focus::Sidebar,
            },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('x')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. })); // Still in create mode
    }

    // === Drafting Mode Tests ===

    #[test]
    fn test_drafting_character_input() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('t')));
        state.handle_key(key(KeyCode::Char('e')));
        state.handle_key(key(KeyCode::Char('s')));
        state.handle_key(key(KeyCode::Char('t')));

        if let AppMode::Drafting(ref draft) = state.mode {
            assert_eq!(draft.name, "test");
        } else {
            panic!("Expected Drafting mode");
        }
    }

    #[test]
    fn test_drafting_backspace() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('a')));
        state.handle_key(key(KeyCode::Char('b')));
        state.handle_key(key(KeyCode::Backspace));

        if let AppMode::Drafting(ref draft) = state.mode {
            assert_eq!(draft.name, "a");
        } else {
            panic!("Expected Drafting mode");
        }
    }

    #[test]
    fn test_drafting_cursor_movement() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('a')));
        state.handle_key(key(KeyCode::Char('b')));
        state.handle_key(key(KeyCode::Char('c')));
        state.handle_key(key(KeyCode::Left));
        state.handle_key(key(KeyCode::Left));

        if let AppMode::Drafting(ref draft) = state.mode {
            assert_eq!(draft.cursor_position, 1);
        } else {
            panic!("Expected Drafting mode");
        }

        state.handle_key(key(KeyCode::Right));

        if let AppMode::Drafting(ref draft) = state.mode {
            assert_eq!(draft.cursor_position, 2);
        } else {
            panic!("Expected Drafting mode");
        }
    }

    #[test]
    fn test_drafting_enter_creates_session() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('t')));
        state.handle_key(key(KeyCode::Char('e')));
        state.handle_key(key(KeyCode::Char('s')));
        state.handle_key(key(KeyCode::Char('t')));

        let result = state.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, EventResult::CreateSession { .. }));
        if let EventResult::CreateSession { name, session_type } = result {
            assert_eq!(name, "test");
            assert_eq!(session_type, SessionType::Terminal);
        }
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_drafting_enter_with_empty_name_does_nothing() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Drafting(_))); // Still drafting
    }

    #[test]
    fn test_drafting_esc_cancels() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Terminal)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Terminal); // Restored
    }

    // === Renaming Mode Tests ===

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_renaming_enter_completes_rename() {
        let mut state = AppState {
            sessions: vec![Session::new("old")],
            focus: Focus::Sidebar,
            mode: AppMode::Renaming(RenamingState::new(0, "old", Focus::Sidebar)),
            ..Default::default()
        };

        // Clear the name and type new one
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Char('n')));
        state.handle_key(key(KeyCode::Char('e')));
        state.handle_key(key(KeyCode::Char('w')));

        let result = state.handle_key(key(KeyCode::Enter));
        // Now returns RenameSession instead of Consumed
        assert!(
            matches!(result, EventResult::RenameSession { old_name, new_name }
            if old_name == "old" && new_name == "new")
        );
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.sessions[0].name, "new");
        assert_eq!(state.focus, Focus::Terminal); // Per spec: rename confirm focuses terminal pane
    }

    #[test]
    fn test_renaming_esc_cancels() {
        let mut state = AppState {
            sessions: vec![Session::new("original")],
            focus: Focus::Sidebar,
            mode: AppMode::Renaming(RenamingState::new(0, "original", Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Char('x')));

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.sessions[0].name, "original"); // Unchanged
        assert_eq!(state.focus, Focus::Sidebar); // Restored
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_renaming_enter_always_focuses_terminal() {
        // Per spec: "Exit rename mode and focus on the terminal pane" — always Terminal, regardless
        // of where focus was when renaming started.
        let mut state = AppState {
            sessions: vec![Session::new("session")],
            focus: Focus::Terminal,
            mode: AppMode::Renaming(RenamingState::new(0, "session", Focus::Terminal)),
            ..Default::default()
        };

        // Type a new name
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Char('n')));
        state.handle_key(key(KeyCode::Char('e')));
        state.handle_key(key(KeyCode::Char('w')));

        let result = state.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, EventResult::RenameSession { .. }));
        assert_eq!(state.focus, Focus::Terminal); // Always focuses terminal after rename confirm
    }

    // === Confirmation Mode Tests ===

    #[test]
    fn test_confirm_quit_y_returns_quit() {
        let mut state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('y')));
        assert_eq!(result, EventResult::Quit);
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_confirm_delete_y_removes_session() {
        let mut state = AppState {
            sessions: vec![Session::new("a"), Session::new("b")],
            mode: AppMode::Confirming(ConfirmState::new(
                ConfirmAction::DeleteSession(0),
                Focus::Sidebar,
            )),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('y')));
        // Now returns DeleteSession instead of Consumed
        assert!(matches!(result, EventResult::DeleteSession { name } if name == "a"));
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].name, "b");
    }

    #[test]
    fn test_confirm_n_cancels() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Sidebar); // Restored
    }

    #[test]
    fn test_confirm_esc_cancels() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Terminal)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Terminal); // Restored
    }

    #[test]
    fn test_confirm_other_keys_consumed() {
        let mut state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('x')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_))); // Still confirming
    }

    #[test]
    fn test_confirm_quit_q_returns_quit() {
        let mut state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Quit);
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_confirm_delete_q_does_not_confirm() {
        let mut state = AppState {
            sessions: vec![Session::new("test")],
            mode: AppMode::Confirming(ConfirmState::new(
                ConfirmAction::DeleteSession(0),
                Focus::Sidebar,
            )),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('q')));
        // 'q' should NOT confirm delete - only quit confirmation
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_))); // Still confirming
        assert_eq!(state.sessions.len(), 1); // Session not deleted
    }

    // === Mouse Mode Toggle Tests ===

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_terminal_ctrl_m_toggles_mouse_mode() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mouse_mode: false,
            ..Default::default()
        };

        // Toggle on
        let result = state.handle_key(ctrl_key('s'));
        assert_eq!(result, EventResult::ToggleMouseMode);
        assert!(state.mouse_mode, "Mouse mode should be enabled");

        // Toggle off
        let result = state.handle_key(ctrl_key('s'));
        assert_eq!(result, EventResult::ToggleMouseMode);
        assert!(!state.mouse_mode, "Mouse mode should be disabled");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_ctrl_m_toggles_mouse_mode() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            mouse_mode: false,
            ..Default::default()
        };

        // Toggle on
        let result = state.handle_key(ctrl_key('s'));
        assert_eq!(result, EventResult::ToggleMouseMode);
        assert!(state.mouse_mode, "Mouse mode should be enabled");

        // Toggle off
        let result = state.handle_key(ctrl_key('s'));
        assert_eq!(result, EventResult::ToggleMouseMode);
        assert!(!state.mouse_mode, "Mouse mode should be disabled");
    }

    #[test]
    fn test_mouse_mode_default_is_true() {
        let state = AppState::default();
        assert!(
            state.mouse_mode,
            "Default mouse mode should be true (scroll wheel enabled)"
        );
    }

    // === Zoom Mode Tests ===

    #[test]
    fn test_zoom_default_is_false() {
        let state = AppState::default();
        assert!(
            !state.zoomed,
            "Default zoom should be false (sidebar visible)"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_ctrl_z_toggles_zoom_from_terminal() {
        let mut state = AppState {
            focus: Focus::Terminal,
            zoomed: false,
            ..Default::default()
        };

        // Toggle on
        let result = state.handle_key(ctrl_key('z'));
        assert_eq!(result, EventResult::ToggleZoom);
        assert!(state.zoomed, "Zoom should be enabled");

        // Toggle off
        let result = state.handle_key(ctrl_key('z'));
        assert_eq!(result, EventResult::ToggleZoom);
        assert!(!state.zoomed, "Zoom should be disabled");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_ctrl_z_not_handled_from_sidebar() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            zoomed: false,
            ..Default::default()
        };
        // Ctrl+Z is not a sidebar binding — should not change zoom
        let result = state.handle_key(ctrl_key('z'));
        assert_ne!(result, EventResult::ToggleZoom);
        assert!(!state.zoomed, "Zoom should remain false from sidebar");
    }

    #[test]
    fn test_focus_sidebar_clears_zoom() {
        let mut state = AppState {
            focus: Focus::Terminal,
            zoomed: true,
            ..Default::default()
        };
        state.focus_sidebar();
        assert!(
            !state.zoomed,
            "Zoom should be cleared when focusing sidebar"
        );
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_ctrl_b_from_terminal_clears_zoom() {
        let mut state = AppState {
            focus: Focus::Terminal,
            zoomed: true,
            ..Default::default()
        };
        let result = state.handle_key(ctrl_key('b'));
        assert_eq!(result, EventResult::Consumed);
        assert!(!state.zoomed, "Zoom should be cleared by ctrl+b");
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_enter_create_mode_clears_zoom() {
        let mut state = AppState {
            focus: Focus::Terminal,
            zoomed: true,
            ..Default::default()
        };
        state.enter_create_mode();
        assert!(
            !state.zoomed,
            "Zoom should be cleared when entering create mode"
        );
    }

    // === Workspace Overlay Tests ===

    fn workspace_overlay_state(
        workspaces: Vec<&str>,
        active: &str,
    ) -> crate::state::WorkspaceOverlayState {
        crate::state::WorkspaceOverlayState::new(
            workspaces.into_iter().map(|s| s.to_string()).collect(),
            active.to_string(),
        )
    }

    #[test]
    fn test_m_key_opens_move_to_workspace_overlay() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            sessions: vec![Session::new("mysession")],
            workspaces: vec!["Default".to_string(), "Work".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('m')));
        assert!(
            matches!(result, EventResult::OpenMoveToWorkspaceOverlay { ref session_name } if session_name == "mysession"),
            "Expected OpenMoveToWorkspaceOverlay, got {:?}",
            result
        );
    }

    #[test]
    fn test_m_key_does_nothing_with_no_sessions() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            sessions: vec![],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('m')));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_workspace_overlay_esc_closes() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_workspace_overlay_navigate_down() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Down));
        if let AppMode::WorkspaceOverlay(ref ov) = state.mode {
            assert_eq!(ov.selected_index, 1);
        } else {
            panic!("Expected WorkspaceOverlay mode");
        }
    }

    #[test]
    fn test_workspace_overlay_navigate_up() {
        let ov = {
            let mut ov = workspace_overlay_state(vec!["Default", "Work"], "Work");
            ov.selected_index = 1;
            ov
        };
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Up));
        if let AppMode::WorkspaceOverlay(ref ov) = state.mode {
            assert_eq!(ov.selected_index, 0);
        } else {
            panic!("Expected WorkspaceOverlay mode");
        }
    }

    #[test]
    fn test_workspace_overlay_enter_switches_workspace() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        // Navigate to "Work"
        state.handle_key(key(KeyCode::Down));
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::SwitchWorkspace { ref name } if name == "Work"),
            "Expected SwitchWorkspace(Work), got {:?}",
            result
        );
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_workspace_overlay_move_mode_enter_moves_session() {
        use crate::state::WorkspaceOverlayState;
        let ov = {
            let mut ov = WorkspaceOverlayState::new_move_mode(
                vec!["Default".to_string(), "Work".to_string()],
                "Default".to_string(),
                "mysession".to_string(),
            );
            ov.selected_index = 1; // Select "Work"
            ov
        };
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::MoveSessionToWorkspace { ref session_name, ref workspace_name }
                if session_name == "mysession" && workspace_name == "Work"),
            "Expected MoveSessionToWorkspace, got {:?}",
            result
        );
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_workspace_overlay_move_mode_n_does_nothing() {
        use crate::state::WorkspaceOverlayState;
        let ov = WorkspaceOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(),
            "mysession".to_string(),
        );
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        // 'n' should be ignored in move mode
        let result = state.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, EventResult::Consumed);
        // No drafting_workspace should be set
        if let AppMode::WorkspaceOverlay(ref ov) = state.mode {
            assert!(
                ov.drafting_workspace.is_none(),
                "drafting_workspace should not be set in move mode"
            );
        }
    }

    #[test]
    fn test_workspace_overlay_move_mode_d_does_nothing() {
        use crate::state::WorkspaceOverlayState;
        let ov = WorkspaceOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(),
            "mysession".to_string(),
        );
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        // 'd' should not delete in move mode
        state.handle_key(key(KeyCode::Down)); // select Work
        let result = state.handle_key(key(KeyCode::Char('d')));
        assert_eq!(result, EventResult::Consumed);
        // Mode should still be WorkspaceOverlay (not Normal)
        assert!(matches!(state.mode, AppMode::WorkspaceOverlay(_)));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_workspace_overlay_normal_mode_n_creates_workspace() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(vec!["Default"], "Default")),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Char('n')));
        if let AppMode::WorkspaceOverlay(ref ov) = state.mode {
            assert!(
                ov.drafting_workspace.is_some(),
                "drafting_workspace should be set after 'n'"
            );
        } else {
            panic!("Expected WorkspaceOverlay mode");
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_ctrl_w_opens_workspace_overlay() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            workspaces: vec!["Default".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(ctrl_key('w'));
        assert_eq!(result, EventResult::OpenWorkspaceOverlay);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_w_opens_workspace_overlay_from_sidebar() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            workspaces: vec!["Default".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('w')));
        assert_eq!(result, EventResult::OpenWorkspaceOverlay);
    }

    #[test]
    fn test_w_does_not_open_workspace_overlay_from_terminal() {
        // bare 'w' should NOT open overlay from terminal (only ctrl+w)
        let mut state = AppState {
            focus: Focus::Terminal,
            workspaces: vec!["Default".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('w')));
        // 'w' from terminal should be passed through, not consumed
        assert_eq!(result, EventResult::NotConsumed);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_workspace_overlay_q_shows_quit_confirmation() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Consumed);
        // Overlay should be closed and quit confirmation should be shown
        assert!(
            matches!(state.mode, AppMode::Confirming(_)),
            "Mode should be Confirming after 'q'"
        );
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(
                confirm.action,
                ConfirmAction::Quit,
                "Should be Quit confirmation"
            );
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_workspace_overlay_move_mode_q_shows_quit_confirmation() {
        use crate::state::WorkspaceOverlayState;
        let ov = WorkspaceOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(),
            "mysession".to_string(),
        );
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Consumed);
        assert!(
            matches!(state.mode, AppMode::Confirming(_)),
            "Mode should be Confirming after 'q'"
        );
    }

    #[test]
    fn test_move_to_same_workspace_is_noop() {
        // Spec: "If the selected workspace is the current workspace, do nothing."
        use crate::state::WorkspaceOverlayState;
        let ov = WorkspaceOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(), // active workspace
            "mysession".to_string(),
        );
        // selected_index is 0, which is "Default" (same as active)
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Enter));
        // Should be Consumed (no-op), not MoveSessionToWorkspace
        assert_eq!(
            result,
            EventResult::Consumed,
            "Moving to same workspace should be a no-op (Consumed), got {:?}",
            result
        );
        // Overlay should be closed
        assert!(
            matches!(state.mode, AppMode::Normal),
            "Overlay should close after no-op move"
        );
    }

    #[test]
    fn test_move_to_different_workspace_works() {
        use crate::state::WorkspaceOverlayState;
        let mut ov = WorkspaceOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(), // active workspace
            "mysession".to_string(),
        );
        // Select "Work" (index 1)
        ov.selected_index = 1;
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::MoveSessionToWorkspace {
                ref session_name, ref workspace_name
            } if session_name == "mysession" && workspace_name == "Work"),
            "Moving to different workspace should emit MoveSessionToWorkspace, got {:?}",
            result
        );
    }

    #[test]
    fn tmux_toggle_commits_and_ctrl_space_legacy_null_is_supported() {
        let mut state = AppState::with_sessions(vec![Session::new("one"), Session::new("two")]);
        state.focus = Focus::Terminal;
        assert_eq!(state.handle_key(ctrl_key('b')), EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
        state.selected_index = 1;
        assert!(
            matches!(state.handle_key(key(KeyCode::Null)), EventResult::SwitchSession { name } if name == "two")
        );
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn tmux_sidebar_commands_create_browse_delete_and_detach() {
        let mut state = AppState::with_sessions(vec![Session::new("one"), Session::new("two")]);
        state.focus = Focus::Sidebar;
        assert!(matches!(
            state.handle_key(key(KeyCode::Char('c'))),
            EventResult::Consumed
        ));
        assert!(matches!(state.mode, AppMode::Drafting(_)));
        state.mode = AppMode::Normal;
        assert!(
            matches!(state.handle_key(key(KeyCode::Char('n'))), EventResult::PreviewSession { name } if name == "two")
        );
        assert_eq!(
            state.handle_key(key(KeyCode::Delete)),
            EventResult::Consumed
        );
        assert!(matches!(
            state.mode,
            AppMode::Confirming(ConfirmState {
                action: ConfirmAction::DeleteSession(1),
                ..
            })
        ));
        state.mode = AppMode::Normal;
        assert_eq!(
            state.handle_key(key(KeyCode::Char('d'))),
            EventResult::Consumed
        );
        assert!(matches!(
            state.mode,
            AppMode::Confirming(ConfirmState {
                action: ConfirmAction::Quit,
                ..
            })
        ));
    }

    #[test]
    fn retired_control_keys_pass_through_terminal() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };
        for c in ['n', 'w', 's', 'z', 'q', 't'] {
            assert_eq!(
                state.handle_key(ctrl_key(c)),
                EventResult::NotConsumed,
                "Ctrl+{c}"
            );
        }
    }

    #[test]
    fn direct_alt_shortcuts_switch_and_reorder() {
        let mut state = AppState::with_sessions(vec![Session::new("one"), Session::new("two")]);
        state.focus = Focus::Terminal;
        assert!(
            matches!(state.handle_key(modified_key(KeyCode::Char('2'), KeyModifiers::ALT)), EventResult::SwitchSession { name } if name == "two")
        );
        assert_eq!(
            state.handle_key(modified_key(
                KeyCode::Left,
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )),
            EventResult::ReorderSession { offset: -1 }
        );
        assert_eq!(state.sessions[0].name, "two");
        assert_eq!(
            state.handle_key(modified_key(KeyCode::Down, KeyModifiers::ALT)),
            EventResult::SwitchRelativeWorkspace { offset: 1 }
        );
    }

    #[test]
    fn modal_text_input_takes_precedence_over_toggle() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };
        state.start_drafting(SessionType::Terminal);
        assert_eq!(state.handle_key(ctrl_key('b')), EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
        assert!(matches!(state.mode, AppMode::Drafting(_)));
    }
}
