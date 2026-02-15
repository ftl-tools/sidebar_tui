//! Keyboard event handling with state machine integration.
//!
//! This module contains the input handler that routes key events based on the
//! current application mode and focus state.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{
    AppMode, AppState, ConfirmAction, EventResult, Focus, SessionType,
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
        // Handle modifier keys
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('n') => {
                    self.enter_create_mode();
                    EventResult::Consumed
                }
                _ => EventResult::NotConsumed,
            };
        }

        match key.code {
            // Navigation
            KeyCode::Up => {
                self.select_previous();
                EventResult::Consumed
            }
            KeyCode::Down => {
                self.select_next();
                EventResult::Consumed
            }

            // Select (focus terminal)
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => {
                if !self.sessions.is_empty() {
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
                // New session
                KeyCode::Char('n') => {
                    self.enter_create_mode();
                    EventResult::Consumed
                }
                _ => EventResult::NotConsumed,
            };
        }

        // All other keys are passed to the terminal (not consumed by state machine)
        EventResult::NotConsumed
    }

    /// Handle key events in create mode (selecting session type: t or a).
    fn handle_create_mode_key(&mut self, key: KeyEvent) -> EventResult {
        match key.code {
            // Terminal session
            KeyCode::Char('t') => {
                self.start_drafting(SessionType::Terminal);
                EventResult::Consumed
            }
            // Agent session
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
                    if !new_name.is_empty() {
                        self.rename_session(index, new_name);
                    }
                    self.mode = AppMode::Normal;
                    self.focus_terminal();
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

    /// Handle key events for confirmation prompts.
    fn handle_confirming_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Confirming(ref confirm) = self.mode {
            match key.code {
                // Confirm (yes)
                KeyCode::Char('y') => {
                    let action = confirm.action.clone();
                    self.mode = AppMode::Normal;
                    match action {
                        ConfirmAction::Quit => EventResult::Quit,
                        ConfirmAction::DeleteSession(index) => {
                            self.remove_session(index);
                            EventResult::Consumed
                        }
                    }
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
    use crate::state::Session;

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
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selected_index, 1);

        state.handle_key(key(KeyCode::Down));
        assert_eq!(state.selected_index, 2);

        // At bottom, should stay
        state.handle_key(key(KeyCode::Down));
        assert_eq!(state.selected_index, 2);

        let result = state.handle_key(key(KeyCode::Up));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selected_index, 1);

        state.handle_key(key(KeyCode::Up));
        assert_eq!(state.selected_index, 0);

        // At top, should stay
        state.handle_key(key(KeyCode::Up));
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_sidebar_enter_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_sidebar_space_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_sidebar_right_focuses_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Right));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_sidebar_enter_does_nothing_when_empty() {
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;

        state.handle_key(key(KeyCode::Enter));
        assert_eq!(state.focus, Focus::Sidebar); // Should stay on sidebar
    }

    #[test]
    fn test_sidebar_esc_jump_back() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
        ]);
        state.focus = Focus::Sidebar;
        state.selected_index = 0;
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
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. }));
    }

    #[test]
    fn test_sidebar_ctrl_n_enters_create_mode() {
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;

        let result = state.handle_key(ctrl_key('n'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. }));
    }

    #[test]
    fn test_sidebar_d_requests_delete_confirmation() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char('d')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert!(matches!(confirm.action, ConfirmAction::DeleteSession(0)));
        }
    }

    #[test]
    fn test_sidebar_d_does_nothing_when_empty() {
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;

        state.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_sidebar_r_starts_renaming() {
        let mut state = AppState::with_sessions(vec![Session::new("old_name")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char('r')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Renaming(_)));
    }

    #[test]
    fn test_sidebar_r_does_nothing_when_empty() {
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;

        state.handle_key(key(KeyCode::Char('r')));
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_sidebar_q_requests_quit_confirmation() {
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;

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
        let mut state = AppState::default();
        state.focus = Focus::Terminal;

        let result = state.handle_key(ctrl_key('b'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_terminal_ctrl_t_focuses_sidebar() {
        let mut state = AppState::default();
        state.focus = Focus::Terminal;

        let result = state.handle_key(ctrl_key('t'));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_terminal_ctrl_n_enters_create_mode() {
        let mut state = AppState::default();
        state.focus = Focus::Terminal;

        let result = state.handle_key(ctrl_key('n'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. }));
    }

    #[test]
    fn test_terminal_regular_keys_not_consumed() {
        let mut state = AppState::default();
        state.focus = Focus::Terminal;

        let result = state.handle_key(key(KeyCode::Char('a')));
        assert_eq!(result, EventResult::NotConsumed);

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::NotConsumed);
    }

    // === Create Mode Tests ===

    #[test]
    fn test_create_mode_t_starts_terminal_drafting() {
        let mut state = AppState::default();
        state.enter_create_mode();

        let result = state.handle_key(key(KeyCode::Char('t')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Drafting(_)));
        if let AppMode::Drafting(ref draft) = state.mode {
            assert_eq!(draft.session_type, SessionType::Terminal);
        }
    }

    #[test]
    fn test_create_mode_a_starts_agent_drafting() {
        let mut state = AppState::default();
        state.enter_create_mode();

        let result = state.handle_key(key(KeyCode::Char('a')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Drafting(_)));
        if let AppMode::Drafting(ref draft) = state.mode {
            assert_eq!(draft.session_type, SessionType::Agent);
        }
    }

    #[test]
    fn test_create_mode_esc_cancels() {
        let mut state = AppState::default();
        state.focus = Focus::Terminal;
        state.enter_create_mode();

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Terminal); // Restored
    }

    #[test]
    fn test_create_mode_other_keys_consumed_but_ignored() {
        let mut state = AppState::default();
        state.enter_create_mode();

        let result = state.handle_key(key(KeyCode::Char('x')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::CreateMode { .. })); // Still in create mode
    }

    // === Drafting Mode Tests ===

    #[test]
    fn test_drafting_character_input() {
        let mut state = AppState::default();
        state.enter_create_mode();
        state.start_drafting(SessionType::Terminal);

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
        let mut state = AppState::default();
        state.enter_create_mode();
        state.start_drafting(SessionType::Terminal);

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
        let mut state = AppState::default();
        state.enter_create_mode();
        state.start_drafting(SessionType::Terminal);

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
        let mut state = AppState::default();
        state.enter_create_mode();
        state.start_drafting(SessionType::Terminal);

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
        let mut state = AppState::default();
        state.enter_create_mode();
        state.start_drafting(SessionType::Terminal);

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Drafting(_))); // Still drafting
    }

    #[test]
    fn test_drafting_esc_cancels() {
        let mut state = AppState::default();
        state.focus = Focus::Terminal;
        state.enter_create_mode();
        state.start_drafting(SessionType::Terminal);

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Terminal); // Restored
    }

    // === Renaming Mode Tests ===

    #[test]
    fn test_renaming_enter_completes_rename() {
        let mut state = AppState::with_sessions(vec![Session::new("old")]);
        state.focus = Focus::Sidebar;
        state.start_renaming();

        // Clear the name and type new one
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Char('n')));
        state.handle_key(key(KeyCode::Char('e')));
        state.handle_key(key(KeyCode::Char('w')));

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.sessions[0].name, "new");
        assert_eq!(state.focus, Focus::Terminal); // Spec says focus terminal after rename
    }

    #[test]
    fn test_renaming_esc_cancels() {
        let mut state = AppState::with_sessions(vec![Session::new("original")]);
        state.focus = Focus::Sidebar;
        state.start_renaming();

        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Char('x')));

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.sessions[0].name, "original"); // Unchanged
        assert_eq!(state.focus, Focus::Sidebar); // Restored
    }

    // === Confirmation Mode Tests ===

    #[test]
    fn test_confirm_quit_y_returns_quit() {
        let mut state = AppState::default();
        state.request_confirmation(ConfirmAction::Quit);

        let result = state.handle_key(key(KeyCode::Char('y')));
        assert_eq!(result, EventResult::Quit);
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_confirm_delete_y_removes_session() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
        ]);
        state.request_confirmation(ConfirmAction::DeleteSession(0));

        let result = state.handle_key(key(KeyCode::Char('y')));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].name, "b");
    }

    #[test]
    fn test_confirm_n_cancels() {
        let mut state = AppState::default();
        state.focus = Focus::Sidebar;
        state.request_confirmation(ConfirmAction::Quit);

        let result = state.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Sidebar); // Restored
    }

    #[test]
    fn test_confirm_esc_cancels() {
        let mut state = AppState::default();
        state.focus = Focus::Terminal;
        state.request_confirmation(ConfirmAction::Quit);

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.focus, Focus::Terminal); // Restored
    }

    #[test]
    fn test_confirm_other_keys_consumed() {
        let mut state = AppState::default();
        state.request_confirmation(ConfirmAction::Quit);

        let result = state.handle_key(key(KeyCode::Char('x')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_))); // Still confirming
    }
}
