//! Keyboard event handling with state machine integration.
//!
//! This module contains the input handler that routes key events based on the
//! current application mode and focus state.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::name_generator::generate_unique_session_name;
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
            AppMode::Normal => {}
        }

        // Normal mode - dispatch based on focus
        match self.focus {
            Focus::Sidebar => self.handle_sidebar_key(key),
            Focus::Terminal => self.handle_terminal_key(key),
        }
    }

    /// Handle key events when sidebar is focused (Normal mode).
    fn handle_sidebar_key(&mut self, key: KeyEvent) -> EventResult {
        // Handle modifier keys - all terminal mod+* commands should work from sidebar
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                // Focus sidebar - no-op when already on sidebar, but consume the key
                KeyCode::Char('b') | KeyCode::Char('t') => EventResult::Consumed,
                // Toggle mouse mode (for text selection vs scroll wheel)
                KeyCode::Char('s') => {
                    self.mouse_mode = !self.mouse_mode;
                    EventResult::ToggleMouseMode
                }
                // New session (create mode)
                KeyCode::Char('n') => {
                    self.enter_create_mode();
                    EventResult::Consumed
                }
                // Quit (show confirmation)
                KeyCode::Char('q') => {
                    self.request_confirmation(ConfirmAction::Quit);
                    EventResult::Consumed
                }
                // Open workspace overlay
                KeyCode::Char('w') => EventResult::OpenWorkspaceOverlay,
                _ => EventResult::NotConsumed,
            };
        }

        match key.code {
            // Navigation (arrows and vim-style j/k) with live preview
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_previous();
                // Return PreviewSession to show terminal content as user navigates
                if let Some(session) = self.sessions.get(self.selected_index) {
                    EventResult::PreviewSession { name: session.name.clone() }
                } else {
                    EventResult::Consumed
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                // Return PreviewSession to show terminal content as user navigates
                if let Some(session) = self.sessions.get(self.selected_index) {
                    EventResult::PreviewSession { name: session.name.clone() }
                } else {
                    EventResult::Consumed
                }
            }

            // Select (focus terminal)
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Tab => {
                if !self.sessions.is_empty() {
                    let name = self.sessions.get(self.selected_index)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    self.focus_terminal();
                    if !name.is_empty() {
                        return EventResult::SwitchSession { name };
                    }
                } else {
                    // In welcome state (no sessions), still allow focusing the terminal
                    // so the welcome text keybinding updates dynamically (shows ctrl+n instead of n).
                    self.focus_terminal();
                }
                EventResult::Consumed
            }

            // Jump back (return to previous session and focus terminal)
            KeyCode::Esc => {
                self.jump_back();
                EventResult::Consumed
            }

            // New session (enter create mode)
            KeyCode::Char('n') => {
                self.enter_create_mode();
                EventResult::Consumed
            }

            // Delete session (show confirmation)
            KeyCode::Char('d') => {
                if !self.sessions.is_empty() {
                    self.request_confirmation(ConfirmAction::DeleteSession(self.selected_index));
                }
                EventResult::Consumed
            }

            // Rename session
            KeyCode::Char('r') => {
                if !self.sessions.is_empty() {
                    self.start_renaming();
                }
                EventResult::Consumed
            }

            // Quit (show confirmation)
            KeyCode::Char('q') => {
                self.request_confirmation(ConfirmAction::Quit);
                EventResult::Consumed
            }

            // Move selected session to another workspace
            KeyCode::Char('m') => {
                if let Some(session) = self.sessions.get(self.selected_index) {
                    let session_name = session.name.clone();
                    return EventResult::OpenMoveToWorkspaceOverlay { session_name };
                }
                EventResult::Consumed
            }

            _ => EventResult::NotConsumed,
        }
    }

    /// Handle key events when terminal is focused (Normal mode).
    fn handle_terminal_key(&mut self, key: KeyEvent) -> EventResult {
        // Only handle modifier key combinations in terminal focus
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                // Focus on sidebar
                KeyCode::Char('b') | KeyCode::Char('t') => {
                    self.focus_sidebar();
                    EventResult::Consumed
                }
                // Toggle mouse mode (for text selection vs scroll wheel)
                KeyCode::Char('s') => {
                    self.mouse_mode = !self.mouse_mode;
                    EventResult::ToggleMouseMode
                }
                // New session
                KeyCode::Char('n') => {
                    self.enter_create_mode();
                    EventResult::Consumed
                }
                // Quit (show confirmation) - works from terminal focus per spec
                KeyCode::Char('q') => {
                    self.request_confirmation(ConfirmAction::Quit);
                    EventResult::Consumed
                }
                // Open workspace overlay
                KeyCode::Char('w') => EventResult::OpenWorkspaceOverlay,
                _ => EventResult::NotConsumed,
            };
        }

        // All other keys are passed to the terminal (not consumed by state machine)
        EventResult::NotConsumed
    }

    /// Handle key events in create mode (selecting session type: t or a).
    /// Directly generates a random session name and creates the session.
    fn handle_create_mode_key(&mut self, key: KeyEvent) -> EventResult {
        match key.code {
            // Terminal session
            KeyCode::Char('t') => {
                let name = self.generate_unique_name();
                self.mode = AppMode::Normal;
                EventResult::CreateSession {
                    name,
                    session_type: SessionType::Terminal,
                }
            }
            // Agent session
            KeyCode::Char('a') => {
                let name = self.generate_unique_name();
                self.mode = AppMode::Normal;
                EventResult::CreateSession {
                    name,
                    session_type: SessionType::Agent,
                }
            }
            // Cancel
            KeyCode::Esc => {
                self.cancel_create_mode();
                EventResult::Consumed
            }
            _ => EventResult::Consumed, // Consume but ignore other keys
        }
    }

    /// Generate a unique session name that doesn't conflict with existing sessions.
    fn generate_unique_name(&self) -> String {
        let existing_names: Vec<&str> = self.sessions.iter().map(|s| s.name.as_str()).collect();
        generate_unique_session_name(&existing_names)
    }

    /// Handle key events while drafting a new session name.
    fn handle_drafting_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Drafting(ref mut draft) = self.mode {
            match key.code {
                // Create the session
                KeyCode::Enter => {
                    let name = draft.name.clone();
                    let session_type = draft.session_type;
                    if !name.is_empty() {
                        // Will be handled by caller to actually create the session
                        self.mode = AppMode::Normal;
                        return EventResult::CreateSession { name, session_type };
                    }
                    EventResult::Consumed
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
                    let previous_focus = rename.previous_focus;
                    // Get old name before updating local state
                    let old_name = self.sessions.get(index)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if !new_name.is_empty() && !old_name.is_empty() {
                        // Update local state
                        self.rename_session(index, new_name.clone());
                        self.mode = AppMode::Normal;
                        // Restore focus to where it was before renaming started
                        self.focus = previous_focus;
                        // Return RenameSession event for daemon to handle
                        return EventResult::RenameSession { old_name, new_name };
                    }
                    self.mode = AppMode::Normal;
                    // Restore focus to where it was before renaming started
                    self.focus = previous_focus;
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
                        let name = overlay.drafting_workspace.as_ref().map(|d| d.new_name.trim().to_string()).unwrap_or_default();
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
                        let new_name = overlay.renaming.as_ref().map(|r| r.new_name.trim().to_string()).unwrap_or_default();
                        if !new_name.is_empty() {
                            let old_name = overlay.workspaces
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
                KeyCode::Esc => {
                    self.mode = AppMode::Normal;
                    return EventResult::Consumed;
                }
                // Navigate down
                KeyCode::Down | KeyCode::Char('j') => {
                    if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                        if ov.selected_index + 1 < ov.workspaces.len() {
                            ov.selected_index += 1;
                        }
                    }
                    return EventResult::Consumed;
                }
                // Navigate up
                KeyCode::Up | KeyCode::Char('k') => {
                    if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                        if ov.selected_index > 0 {
                            ov.selected_index -= 1;
                        }
                    }
                    return EventResult::Consumed;
                }
                // Select (switch to workspace) or move session to workspace
                KeyCode::Enter => {
                    let (mode, selected, workspaces, active_workspace) = if let AppMode::WorkspaceOverlay(ref ov) = self.mode {
                        (ov.mode.clone(), ov.selected_index, ov.workspaces.clone(), ov.active_workspace.clone())
                    } else {
                        return EventResult::Consumed;
                    };
                    let workspace_name = workspaces.get(selected).cloned().unwrap_or_default();
                    self.mode = AppMode::Normal;
                    return match mode {
                        WorkspaceOverlayMode::Normal => EventResult::SwitchWorkspace { name: workspace_name },
                        WorkspaceOverlayMode::MoveSession { session_name } => {
                            // Spec: "If the selected workspace is the current workspace, do nothing."
                            if workspace_name == active_workspace {
                                EventResult::Consumed
                            } else {
                                EventResult::MoveSessionToWorkspace { session_name, workspace_name }
                            }
                        }
                    };
                }
                // Create/rename/delete are disabled in move mode
                KeyCode::Char('n') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::WorkspaceOverlay(ref ov) if matches!(ov.mode, WorkspaceOverlayMode::MoveSession { .. })
                    );
                    if !is_move_mode {
                        if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                            ov.drafting_workspace = Some(RenamingState::new(0, "", self.focus));
                        }
                    }
                    return EventResult::Consumed;
                }
                // Rename selected workspace (disabled in move mode)
                KeyCode::Char('r') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::WorkspaceOverlay(ref ov) if matches!(ov.mode, WorkspaceOverlayMode::MoveSession { .. })
                    );
                    if !is_move_mode {
                        let selected_name = if let AppMode::WorkspaceOverlay(ref ov) = self.mode {
                            ov.workspaces.get(ov.selected_index).cloned().unwrap_or_default()
                        } else {
                            String::new()
                        };
                        if !selected_name.is_empty() {
                            if let AppMode::WorkspaceOverlay(ref mut ov) = self.mode {
                                ov.renaming = Some(RenamingState::new(0, &selected_name, Focus::Sidebar));
                            }
                        }
                    }
                    return EventResult::Consumed;
                }
                // Delete selected workspace (disabled in move mode) - shows confirmation
                KeyCode::Char('d') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::WorkspaceOverlay(ref ov) if matches!(ov.mode, WorkspaceOverlayMode::MoveSession { .. })
                    );
                    if !is_move_mode {
                        let (selected, workspaces, active) = if let AppMode::WorkspaceOverlay(ref ov) = self.mode {
                            (ov.selected_index, ov.workspaces.clone(), ov.active_workspace.clone())
                        } else {
                            return EventResult::Consumed;
                        };
                        let workspace_name = workspaces.get(selected).cloned().unwrap_or_default();
                        // Don't delete the last workspace or the active workspace
                        if workspace_name.is_empty() || workspaces.len() <= 1 {
                            return EventResult::Consumed;
                        }
                        // Close overlay and show confirmation
                        self.mode = AppMode::Confirming(ConfirmState::new(
                            ConfirmAction::DeleteWorkspace(workspace_name),
                            if active == "Default" { self.focus } else { self.focus },
                        ));
                    }
                    return EventResult::Consumed;
                }
                // Quit - show confirmation prompt (same as sidebar)
                KeyCode::Char('q') => {
                    self.mode = AppMode::Normal;
                    self.request_confirmation(ConfirmAction::Quit);
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
            let is_quit_confirm = matches!(confirm.action, ConfirmAction::Quit)
                && key.code == KeyCode::Char('q');

            match key.code {
                // Confirm (yes, or 'q' for quit specifically)
                KeyCode::Char('y') => {
                    let action = confirm.action.clone();
                    self.mode = AppMode::Normal;
                    match action {
                        ConfirmAction::Quit => EventResult::Quit,
                        ConfirmAction::DeleteSession(index) => {
                            // Get the session name before removing from local state
                            let name = self.sessions.get(index)
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
    use crate::state::{Session, DraftingState, RenamingState, ConfirmState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
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
    fn test_sidebar_space_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char(' ')));
        // Now returns SwitchSession instead of Consumed
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_sidebar_right_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Right));
        // Now returns SwitchSession instead of Consumed
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_sidebar_tab_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Tab));
        // Tab should focus terminal just like Enter, Space, and Right
        assert!(matches!(result, EventResult::SwitchSession { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
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
    fn test_sidebar_ctrl_b_is_noop() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        // Ctrl+B from sidebar should be consumed (no-op, already on sidebar)
        let result = state.handle_key(ctrl_key('b'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar); // Still on sidebar
    }

    #[test]
    fn test_sidebar_ctrl_t_is_noop() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        // Ctrl+T from sidebar should be consumed (no-op, already on sidebar)
        let result = state.handle_key(ctrl_key('t'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar); // Still on sidebar
    }

    #[test]
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
    fn test_create_mode_t_creates_terminal_session() {
        let mut state = AppState {
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('t')));
        // Should directly create session with auto-generated name
        match result {
            EventResult::CreateSession { name, session_type } => {
                assert!(!name.is_empty(), "Name should be auto-generated");
                assert_eq!(session_type, SessionType::Terminal);
                // Name should be three words with first capitalized
                assert!(name.chars().next().unwrap().is_uppercase(),
                        "First char should be uppercase: {}", name);
            }
            _ => panic!("Expected CreateSession, got {:?}", result),
        }
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_create_mode_a_creates_agent_session() {
        let mut state = AppState {
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('a')));
        // Should directly create session with auto-generated name
        match result {
            EventResult::CreateSession { name, session_type } => {
                assert!(!name.is_empty(), "Name should be auto-generated");
                assert_eq!(session_type, SessionType::Agent);
                // Name should be three words with first capitalized
                assert!(name.chars().next().unwrap().is_uppercase(),
                        "First char should be uppercase: {}", name);
            }
            _ => panic!("Expected CreateSession, got {:?}", result),
        }
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_create_mode_esc_cancels() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::CreateMode { previous_focus: Focus::Terminal },
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
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('x')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. })); // Still in create mode
    }

    #[test]
    fn test_create_mode_generates_unique_name() {
        // Create state with some existing sessions
        let mut state = AppState {
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            sessions: vec![
                Session::new("Test session"),
                Session::new("Another session"),
            ],
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('t')));
        match result {
            EventResult::CreateSession { name, .. } => {
                // Name should not match any existing session (case-insensitive)
                let name_lower = name.to_lowercase();
                assert_ne!(name_lower, "test session");
                assert_ne!(name_lower, "another session");
            }
            _ => panic!("Expected CreateSession"),
        }
    }

    #[test]
    fn test_generate_unique_name_helper() {
        let state = AppState {
            sessions: vec![Session::new("Existing")],
            ..Default::default()
        };

        let name = state.generate_unique_name();
        assert!(!name.is_empty());
        assert!(name.chars().next().unwrap().is_uppercase());
        // Should have 2 spaces (3 words)
        assert_eq!(name.matches(' ').count(), 2);
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
        assert!(matches!(result, EventResult::RenameSession { old_name, new_name }
            if old_name == "old" && new_name == "new"));
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.sessions[0].name, "new");
        assert_eq!(state.focus, Focus::Sidebar); // Focus restored to where it was before rename
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
    fn test_renaming_from_terminal_restores_to_terminal() {
        // Test that rename restores focus to Terminal if that's where it started
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
        assert_eq!(state.focus, Focus::Terminal); // Restored to Terminal
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
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar)),
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
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar)),
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
    fn test_mouse_mode_default_is_false() {
        let state = AppState::default();
        assert!(!state.mouse_mode, "Default mouse mode should be false (text selection enabled)");
    }

    // === Workspace Overlay Tests ===

    fn workspace_overlay_state(workspaces: Vec<&str>, active: &str) -> crate::state::WorkspaceOverlayState {
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
            "Expected OpenMoveToWorkspaceOverlay, got {:?}", result
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
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(vec!["Default", "Work"], "Default")),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_workspace_overlay_navigate_down() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(vec!["Default", "Work"], "Default")),
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
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(vec!["Default", "Work"], "Default")),
            ..Default::default()
        };
        // Navigate to "Work"
        state.handle_key(key(KeyCode::Down));
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::SwitchWorkspace { ref name } if name == "Work"),
            "Expected SwitchWorkspace(Work), got {:?}", result
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
            "Expected MoveSessionToWorkspace, got {:?}", result
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
            assert!(ov.drafting_workspace.is_none(), "drafting_workspace should not be set in move mode");
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
    fn test_workspace_overlay_normal_mode_n_creates_workspace() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(vec!["Default"], "Default")),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Char('n')));
        if let AppMode::WorkspaceOverlay(ref ov) = state.mode {
            assert!(ov.drafting_workspace.is_some(), "drafting_workspace should be set after 'n'");
        } else {
            panic!("Expected WorkspaceOverlay mode");
        }
    }

    #[test]
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
    fn test_workspace_overlay_q_shows_quit_confirmation() {
        let mut state = AppState {
            mode: AppMode::WorkspaceOverlay(workspace_overlay_state(vec!["Default", "Work"], "Default")),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Consumed);
        // Overlay should be closed and quit confirmation should be shown
        assert!(matches!(state.mode, AppMode::Confirming(_)), "Mode should be Confirming after 'q'");
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Quit, "Should be Quit confirmation");
        }
    }

    #[test]
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
        assert!(matches!(state.mode, AppMode::Confirming(_)), "Mode should be Confirming after 'q'");
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
        assert!(matches!(state.mode, AppMode::Normal), "Overlay should close after no-op move");
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
}
