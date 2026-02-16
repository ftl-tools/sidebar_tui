//! Application state management for Sidebar TUI.
//!
//! This module defines the core state types for managing focus, modes, and sessions
//! following patterns from gitui, Zellij, and ratatui examples.

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// Sidebar pane is focused (session list).
    #[default]
    Sidebar,
    /// Terminal pane is focused.
    Terminal,
}

/// Type of session being created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    /// Standard terminal session.
    Terminal,
    /// Agent session (runs `claude` command on creation).
    Agent,
}

/// State for drafting a new session name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftingState {
    /// Type of session being created (Terminal or Agent).
    pub session_type: SessionType,
    /// Current draft name being typed.
    pub name: String,
    /// Cursor position within the name (byte index).
    pub cursor_position: usize,
    /// Focus before entering create mode (to restore on cancel).
    pub previous_focus: Focus,
}

impl DraftingState {
    /// Create a new DraftingState for the given session type.
    pub fn new(session_type: SessionType, previous_focus: Focus) -> Self {
        Self {
            session_type,
            name: String::new(),
            cursor_position: 0,
            previous_focus,
        }
    }

    /// Insert a character at the cursor position if it's a valid session name character.
    /// Valid characters: a-z, A-Z, 0-9, space, hyphen, underscore, period.
    pub fn insert_char(&mut self, c: char) {
        if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' || c == '.' {
            self.name.insert(self.cursor_position, c);
            self.cursor_position += c.len_utf8();
        }
    }

    /// Delete the character before the cursor (backspace).
    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            // Find the previous character boundary
            let mut prev_pos = self.cursor_position - 1;
            while prev_pos > 0 && !self.name.is_char_boundary(prev_pos) {
                prev_pos -= 1;
            }
            self.name.remove(prev_pos);
            self.cursor_position = prev_pos;
        }
    }

    /// Move cursor left by one character.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            let mut prev_pos = self.cursor_position - 1;
            while prev_pos > 0 && !self.name.is_char_boundary(prev_pos) {
                prev_pos -= 1;
            }
            self.cursor_position = prev_pos;
        }
    }

    /// Move cursor right by one character.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.name.len() {
            let mut next_pos = self.cursor_position + 1;
            while next_pos < self.name.len() && !self.name.is_char_boundary(next_pos) {
                next_pos += 1;
            }
            self.cursor_position = next_pos;
        }
    }
}

/// State for renaming an existing session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamingState {
    /// Index of the session being renamed in the sessions list.
    pub session_index: usize,
    /// New name being typed.
    pub new_name: String,
    /// Cursor position within the new name (byte index).
    pub cursor_position: usize,
    /// Focus before starting rename (to restore on cancel).
    pub previous_focus: Focus,
}

impl RenamingState {
    /// Create a new RenamingState from an existing session.
    pub fn new(session_index: usize, current_name: &str, previous_focus: Focus) -> Self {
        let cursor_position = current_name.len();
        Self {
            session_index,
            new_name: current_name.to_string(),
            cursor_position,
            previous_focus,
        }
    }

    /// Insert a character at the cursor position if it's a valid session name character.
    pub fn insert_char(&mut self, c: char) {
        if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' || c == '.' {
            self.new_name.insert(self.cursor_position, c);
            self.cursor_position += c.len_utf8();
        }
    }

    /// Delete the character before the cursor (backspace).
    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            let mut prev_pos = self.cursor_position - 1;
            while prev_pos > 0 && !self.new_name.is_char_boundary(prev_pos) {
                prev_pos -= 1;
            }
            self.new_name.remove(prev_pos);
            self.cursor_position = prev_pos;
        }
    }

    /// Move cursor left by one character.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            let mut prev_pos = self.cursor_position - 1;
            while prev_pos > 0 && !self.new_name.is_char_boundary(prev_pos) {
                prev_pos -= 1;
            }
            self.cursor_position = prev_pos;
        }
    }

    /// Move cursor right by one character.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.new_name.len() {
            let mut next_pos = self.cursor_position + 1;
            while next_pos < self.new_name.len() && !self.new_name.is_char_boundary(next_pos) {
                next_pos += 1;
            }
            self.cursor_position = next_pos;
        }
    }
}

/// Action that requires confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Delete a session by its index.
    DeleteSession(usize),
    /// Quit the TUI.
    Quit,
}

/// State for showing a confirmation prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmState {
    /// Action that will be performed if confirmed.
    pub action: ConfirmAction,
    /// Focus before entering confirmation (to restore on cancel).
    pub previous_focus: Focus,
}

impl ConfirmState {
    /// Create a new ConfirmState for the given action.
    pub fn new(action: ConfirmAction, previous_focus: Focus) -> Self {
        Self {
            action,
            previous_focus,
        }
    }

    /// Get the confirmation prompt message.
    pub fn message(&self) -> &'static str {
        match &self.action {
            ConfirmAction::DeleteSession(_) => "Delete this session permanently?",
            ConfirmAction::Quit => "Quit Sidebar TUI?",
        }
    }

    /// Check if this confirmation should show as important (red background).
    pub fn is_important(&self) -> bool {
        matches!(&self.action, ConfirmAction::DeleteSession(_))
    }
}

/// Application mode - determines what input mode the TUI is in.
/// Modal states take precedence over focus-based input handling.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AppMode {
    /// Normal operation - input depends on current focus.
    #[default]
    Normal,
    /// Create mode - waiting for user to select session type (t or a).
    CreateMode {
        /// Focus before entering create mode (to restore on cancel).
        previous_focus: Focus,
    },
    /// Drafting a new session name.
    Drafting(DraftingState),
    /// Renaming an existing session.
    Renaming(RenamingState),
    /// Showing a confirmation prompt.
    Confirming(ConfirmState),
}

impl AppMode {
    /// Check if we're in any text input mode (drafting or renaming).
    pub fn is_text_input(&self) -> bool {
        matches!(self, AppMode::Drafting(_) | AppMode::Renaming(_))
    }

    /// Check if we're in a modal state (not Normal).
    pub fn is_modal(&self) -> bool {
        !matches!(self, AppMode::Normal)
    }
}

/// A terminal session in the sidebar list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Session name displayed in the sidebar.
    pub name: String,
    /// Whether this session is currently attached (active in this TUI).
    pub is_attached: bool,
}

impl Session {
    /// Create a new Session with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_attached: false,
        }
    }

    /// Create a new attached Session.
    pub fn attached(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_attached: true,
        }
    }
}

/// Result of handling a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventResult {
    /// Event was consumed and handled.
    Consumed,
    /// Event was not consumed (may be handled by caller).
    NotConsumed,
    /// User requested to quit the TUI.
    Quit,
    /// Create a new session with the given name and type.
    CreateSession {
        /// Name for the new session.
        name: String,
        /// Type of session (Terminal or Agent).
        session_type: SessionType,
    },
    /// Delete a session by name.
    DeleteSession {
        /// Name of the session to delete.
        name: String,
    },
    /// Rename a session.
    RenameSession {
        /// Old session name.
        old_name: String,
        /// New session name.
        new_name: String,
    },
    /// Switch to a different session by name.
    SwitchSession {
        /// Name of the session to switch to.
        name: String,
    },
    /// Preview a session's terminal content (without fully attaching).
    /// Used when navigating the sidebar to show a live preview.
    PreviewSession {
        /// Name of the session to preview.
        name: String,
    },
}

/// Main application state.
#[derive(Debug, Clone, Default)]
pub struct AppState {
    /// Which pane currently has focus.
    pub focus: Focus,
    /// Current application mode (Normal, CreateMode, Drafting, etc.).
    pub mode: AppMode,
    /// List of terminal sessions (ordered by most recently used).
    pub sessions: Vec<Session>,
    /// Index of the currently selected session in the sidebar.
    pub selected_index: usize,
    /// Scroll offset for the sidebar session list.
    pub scroll_offset: usize,
    /// Previous session index for "Jump Back" on Esc.
    pub previous_session: Option<usize>,
}


impl AppState {
    /// Create a new AppState with the given sessions.
    pub fn with_sessions(sessions: Vec<Session>) -> Self {
        Self {
            sessions,
            ..Default::default()
        }
    }

    /// Check if we're in the welcome state (no sessions).
    pub fn is_welcome_state(&self) -> bool {
        self.sessions.is_empty() && matches!(self.mode, AppMode::Normal)
    }

    /// Get the currently selected session, if any.
    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions.get(self.selected_index)
    }

    /// Move selection up in the sidebar.
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            // Scroll up if necessary
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            }
        }
    }

    /// Move selection down in the sidebar.
    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.sessions.len() {
            self.selected_index += 1;
            // Note: scroll_offset adjustment will be handled during rendering
            // based on visible area height
        }
    }

    /// Focus on the terminal pane.
    pub fn focus_terminal(&mut self) {
        // Remember current session for "Jump Back"
        if !self.sessions.is_empty() {
            self.previous_session = Some(self.selected_index);
        }
        self.focus = Focus::Terminal;
    }

    /// Focus on the sidebar pane.
    pub fn focus_sidebar(&mut self) {
        self.focus = Focus::Sidebar;
    }

    /// Enter create mode from current focus.
    pub fn enter_create_mode(&mut self) {
        self.mode = AppMode::CreateMode {
            previous_focus: self.focus,
        };
    }

    /// Start drafting a new session with the given type.
    pub fn start_drafting(&mut self, session_type: SessionType) {
        let previous_focus = match &self.mode {
            AppMode::CreateMode { previous_focus } => *previous_focus,
            _ => self.focus,
        };
        self.mode = AppMode::Drafting(DraftingState::new(session_type, previous_focus));
        // Focus moves to the draft row in sidebar
        self.focus = Focus::Sidebar;
    }

    /// Cancel drafting and return to previous state.
    pub fn cancel_drafting(&mut self) {
        if let AppMode::Drafting(state) = &self.mode {
            self.focus = state.previous_focus;
        }
        self.mode = AppMode::Normal;
    }

    /// Start renaming the selected session.
    pub fn start_renaming(&mut self) {
        if let Some(session) = self.selected_session() {
            let state = RenamingState::new(
                self.selected_index,
                &session.name,
                self.focus,
            );
            self.mode = AppMode::Renaming(state);
        }
    }

    /// Cancel renaming and return to previous state.
    pub fn cancel_renaming(&mut self) {
        if let AppMode::Renaming(state) = &self.mode {
            self.focus = state.previous_focus;
        }
        self.mode = AppMode::Normal;
    }

    /// Cancel create mode and return to previous state.
    pub fn cancel_create_mode(&mut self) {
        if let AppMode::CreateMode { previous_focus } = &self.mode {
            self.focus = *previous_focus;
        }
        self.mode = AppMode::Normal;
    }

    /// Show confirmation prompt for an action.
    pub fn request_confirmation(&mut self, action: ConfirmAction) {
        self.mode = AppMode::Confirming(ConfirmState::new(action, self.focus));
    }

    /// Cancel confirmation and return to previous state.
    pub fn cancel_confirmation(&mut self) {
        if let AppMode::Confirming(state) = &self.mode {
            self.focus = state.previous_focus;
        }
        self.mode = AppMode::Normal;
    }

    /// Perform "Jump Back" - return to previous session on Esc from sidebar.
    pub fn jump_back(&mut self) {
        if let Some(prev) = self.previous_session {
            if prev < self.sessions.len() {
                self.selected_index = prev;
            }
        }
        self.focus_terminal();
    }

    /// Add a new session to the top of the list.
    pub fn add_session(&mut self, session: Session) {
        self.sessions.insert(0, session);
        // Keep selection on the new session
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Remove a session by index. Returns true if removed.
    pub fn remove_session(&mut self, index: usize) -> bool {
        if index < self.sessions.len() {
            self.sessions.remove(index);
            // Adjust selection if necessary
            if self.selected_index >= self.sessions.len() && !self.sessions.is_empty() {
                self.selected_index = self.sessions.len() - 1;
            }
            // Adjust previous_session
            if let Some(prev) = self.previous_session {
                use std::cmp::Ordering;
                match prev.cmp(&index) {
                    Ordering::Equal => self.previous_session = None,
                    Ordering::Greater => self.previous_session = Some(prev - 1),
                    Ordering::Less => {}
                }
            }
            true
        } else {
            false
        }
    }

    /// Rename a session by index. Returns true if renamed.
    pub fn rename_session(&mut self, index: usize, new_name: String) -> bool {
        if let Some(session) = self.sessions.get_mut(index) {
            session.name = new_name;
            true
        } else {
            false
        }
    }

    /// Move a session to the top of the list (most recently used).
    /// Used when a session becomes active (e.g., user sends input).
    pub fn move_session_to_top(&mut self, index: usize) {
        if index > 0 && index < self.sessions.len() {
            let session = self.sessions.remove(index);
            self.sessions.insert(0, session);
            // Adjust selected_index to keep selection on the same session
            if self.selected_index == index {
                self.selected_index = 0;
            } else if self.selected_index < index {
                // Session was below selection, no adjustment needed
            } else {
                // This shouldn't happen since we only move sessions above the selection
            }
            // Adjust previous_session
            if let Some(prev) = self.previous_session {
                if prev == index {
                    self.previous_session = Some(0);
                } else if prev < index {
                    self.previous_session = Some(prev + 1);
                }
            }
            // Adjust scroll_offset if needed
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.min(self.selected_index);
            }
        }
    }

    /// Move the currently selected session to the top of the list.
    pub fn move_selected_to_top(&mut self) {
        if self.selected_index > 0 && !self.sessions.is_empty() {
            let session = self.sessions.remove(self.selected_index);
            self.sessions.insert(0, session);
            // Adjust previous_session
            if let Some(prev) = self.previous_session {
                if prev == self.selected_index {
                    self.previous_session = Some(0);
                } else if prev < self.selected_index {
                    self.previous_session = Some(prev + 1);
                }
            }
            self.selected_index = 0;
            self.scroll_offset = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Focus tests
    #[test]
    fn test_focus_default_is_sidebar() {
        assert_eq!(Focus::default(), Focus::Sidebar);
    }

    #[test]
    fn test_focus_equality() {
        assert_eq!(Focus::Sidebar, Focus::Sidebar);
        assert_eq!(Focus::Terminal, Focus::Terminal);
        assert_ne!(Focus::Sidebar, Focus::Terminal);
    }

    // SessionType tests
    #[test]
    fn test_session_type_equality() {
        assert_eq!(SessionType::Terminal, SessionType::Terminal);
        assert_eq!(SessionType::Agent, SessionType::Agent);
        assert_ne!(SessionType::Terminal, SessionType::Agent);
    }

    // DraftingState tests
    #[test]
    fn test_drafting_state_new() {
        let state = DraftingState::new(SessionType::Terminal, Focus::Terminal);
        assert_eq!(state.session_type, SessionType::Terminal);
        assert_eq!(state.name, "");
        assert_eq!(state.cursor_position, 0);
        assert_eq!(state.previous_focus, Focus::Terminal);
    }

    #[test]
    fn test_drafting_state_insert_valid_chars() {
        let mut state = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        state.insert_char('a');
        assert_eq!(state.name, "a");
        state.insert_char('B');
        assert_eq!(state.name, "aB");
        state.insert_char('1');
        assert_eq!(state.name, "aB1");
        state.insert_char(' ');
        assert_eq!(state.name, "aB1 ");
        state.insert_char('-');
        assert_eq!(state.name, "aB1 -");
        state.insert_char('_');
        assert_eq!(state.name, "aB1 -_");
        state.insert_char('.');
        assert_eq!(state.name, "aB1 -_.");
        assert_eq!(state.cursor_position, 7);
    }

    #[test]
    fn test_drafting_state_insert_invalid_chars_ignored() {
        let mut state = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        state.insert_char('!');
        assert_eq!(state.name, "");
        state.insert_char('@');
        assert_eq!(state.name, "");
        state.insert_char('/');
        assert_eq!(state.name, "");
        state.insert_char('\\');
        assert_eq!(state.name, "");
    }

    #[test]
    fn test_drafting_state_delete_char() {
        let mut state = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.name, "abc");
        state.delete_char();
        assert_eq!(state.name, "ab");
        assert_eq!(state.cursor_position, 2);
    }

    #[test]
    fn test_drafting_state_delete_at_start_does_nothing() {
        let mut state = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        state.delete_char();
        assert_eq!(state.name, "");
        assert_eq!(state.cursor_position, 0);
    }

    #[test]
    fn test_drafting_state_cursor_movement() {
        let mut state = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.cursor_position, 3);

        state.move_cursor_left();
        assert_eq!(state.cursor_position, 2);

        state.move_cursor_left();
        assert_eq!(state.cursor_position, 1);

        state.move_cursor_right();
        assert_eq!(state.cursor_position, 2);

        // At rightmost, should not move further
        state.move_cursor_right();
        state.move_cursor_right();
        assert_eq!(state.cursor_position, 3);

        // At leftmost, should not move further
        state.move_cursor_left();
        state.move_cursor_left();
        state.move_cursor_left();
        state.move_cursor_left();
        assert_eq!(state.cursor_position, 0);
    }

    // RenamingState tests
    #[test]
    fn test_renaming_state_new() {
        let state = RenamingState::new(2, "old_name", Focus::Sidebar);
        assert_eq!(state.session_index, 2);
        assert_eq!(state.new_name, "old_name");
        assert_eq!(state.cursor_position, 8); // At end
        assert_eq!(state.previous_focus, Focus::Sidebar);
    }

    #[test]
    fn test_renaming_state_insert_and_delete() {
        let mut state = RenamingState::new(0, "test", Focus::Sidebar);
        state.insert_char('!');
        assert_eq!(state.new_name, "test"); // Invalid char ignored
        state.insert_char('X');
        assert_eq!(state.new_name, "testX");
        state.delete_char();
        assert_eq!(state.new_name, "test");
    }

    // ConfirmAction tests
    #[test]
    fn test_confirm_action_equality() {
        assert_eq!(ConfirmAction::Quit, ConfirmAction::Quit);
        assert_eq!(ConfirmAction::DeleteSession(0), ConfirmAction::DeleteSession(0));
        assert_ne!(ConfirmAction::DeleteSession(0), ConfirmAction::DeleteSession(1));
        assert_ne!(ConfirmAction::Quit, ConfirmAction::DeleteSession(0));
    }

    // ConfirmState tests
    #[test]
    fn test_confirm_state_message_quit() {
        let state = ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar);
        assert_eq!(state.message(), "Quit Sidebar TUI?");
    }

    #[test]
    fn test_confirm_state_message_delete() {
        let state = ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar);
        assert_eq!(state.message(), "Delete this session permanently?");
    }

    #[test]
    fn test_confirm_state_is_important() {
        let quit_state = ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar);
        assert!(!quit_state.is_important());

        let delete_state = ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar);
        assert!(delete_state.is_important());
    }

    // AppMode tests
    #[test]
    fn test_app_mode_default_is_normal() {
        assert_eq!(AppMode::default(), AppMode::Normal);
    }

    #[test]
    fn test_app_mode_is_text_input() {
        assert!(!AppMode::Normal.is_text_input());
        assert!(!AppMode::CreateMode { previous_focus: Focus::Sidebar }.is_text_input());
        assert!(AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)).is_text_input());
        assert!(AppMode::Renaming(RenamingState::new(0, "test", Focus::Sidebar)).is_text_input());
        assert!(!AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)).is_text_input());
    }

    #[test]
    fn test_app_mode_is_modal() {
        assert!(!AppMode::Normal.is_modal());
        assert!(AppMode::CreateMode { previous_focus: Focus::Sidebar }.is_modal());
        assert!(AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)).is_modal());
        assert!(AppMode::Renaming(RenamingState::new(0, "test", Focus::Sidebar)).is_modal());
        assert!(AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)).is_modal());
    }

    // Session tests
    #[test]
    fn test_session_new() {
        let session = Session::new("my-session");
        assert_eq!(session.name, "my-session");
        assert!(!session.is_attached);
    }

    #[test]
    fn test_session_attached() {
        let session = Session::attached("main");
        assert_eq!(session.name, "main");
        assert!(session.is_attached);
    }

    // EventResult tests
    #[test]
    fn test_event_result_equality() {
        assert_eq!(EventResult::Consumed, EventResult::Consumed);
        assert_eq!(EventResult::NotConsumed, EventResult::NotConsumed);
        assert_eq!(EventResult::Quit, EventResult::Quit);
        assert_ne!(EventResult::Consumed, EventResult::NotConsumed);
    }

    // AppState tests
    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert_eq!(state.focus, Focus::Sidebar);
        assert_eq!(state.mode, AppMode::Normal);
        assert!(state.sessions.is_empty());
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.scroll_offset, 0);
        assert!(state.previous_session.is_none());
    }

    #[test]
    fn test_app_state_with_sessions() {
        let sessions = vec![
            Session::new("session1"),
            Session::new("session2"),
        ];
        let state = AppState::with_sessions(sessions);
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.sessions[0].name, "session1");
    }

    #[test]
    fn test_app_state_is_welcome_state() {
        let empty_state = AppState::default();
        assert!(empty_state.is_welcome_state());

        let with_sessions = AppState::with_sessions(vec![Session::new("test")]);
        assert!(!with_sessions.is_welcome_state());

        let modal_state = AppState {
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            ..Default::default()
        };
        assert!(!modal_state.is_welcome_state());
    }

    #[test]
    fn test_app_state_selected_session() {
        let state = AppState::default();
        assert!(state.selected_session().is_none());

        let state = AppState::with_sessions(vec![Session::new("test")]);
        assert_eq!(state.selected_session().unwrap().name, "test");
    }

    #[test]
    fn test_app_state_select_navigation() {
        let mut state = AppState::with_sessions(vec![
            Session::new("session1"),
            Session::new("session2"),
            Session::new("session3"),
        ]);

        assert_eq!(state.selected_index, 0);

        state.select_next();
        assert_eq!(state.selected_index, 1);

        state.select_next();
        assert_eq!(state.selected_index, 2);

        // At end, should not go further
        state.select_next();
        assert_eq!(state.selected_index, 2);

        state.select_previous();
        assert_eq!(state.selected_index, 1);

        state.select_previous();
        assert_eq!(state.selected_index, 0);

        // At start, should not go further
        state.select_previous();
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_app_state_focus_terminal() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.selected_index = 0;

        state.focus_terminal();
        assert_eq!(state.focus, Focus::Terminal);
        assert_eq!(state.previous_session, Some(0));
    }

    #[test]
    fn test_app_state_focus_sidebar() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        state.focus_sidebar();
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_app_state_enter_create_mode() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        state.enter_create_mode();
        match &state.mode {
            AppMode::CreateMode { previous_focus } => {
                assert_eq!(*previous_focus, Focus::Terminal);
            }
            _ => panic!("Expected CreateMode"),
        }
    }

    #[test]
    fn test_app_state_start_drafting() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::CreateMode { previous_focus: Focus::Terminal },
            ..Default::default()
        };

        state.start_drafting(SessionType::Agent);
        match &state.mode {
            AppMode::Drafting(draft) => {
                assert_eq!(draft.session_type, SessionType::Agent);
                assert_eq!(draft.previous_focus, Focus::Terminal);
            }
            _ => panic!("Expected Drafting"),
        }
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_app_state_cancel_drafting() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Terminal)),
            ..Default::default()
        };

        state.cancel_drafting();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_app_state_start_renaming() {
        let mut state = AppState::with_sessions(vec![Session::new("old_name")]);
        state.focus = Focus::Sidebar;

        state.start_renaming();
        match &state.mode {
            AppMode::Renaming(rename) => {
                assert_eq!(rename.session_index, 0);
                assert_eq!(rename.new_name, "old_name");
                assert_eq!(rename.previous_focus, Focus::Sidebar);
            }
            _ => panic!("Expected Renaming"),
        }
    }

    #[test]
    fn test_app_state_cancel_renaming() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.focus = Focus::Terminal;
        state.focus_sidebar();
        state.start_renaming();

        state.cancel_renaming();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Sidebar);
    }

    #[test]
    fn test_app_state_request_confirmation() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        state.request_confirmation(ConfirmAction::Quit);
        match &state.mode {
            AppMode::Confirming(confirm) => {
                assert_eq!(confirm.action, ConfirmAction::Quit);
                assert_eq!(confirm.previous_focus, Focus::Sidebar);
            }
            _ => panic!("Expected Confirming"),
        }
    }

    #[test]
    fn test_app_state_cancel_confirmation() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Terminal)),
            ..Default::default()
        };

        state.cancel_confirmation();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_app_state_jump_back() {
        let mut state = AppState::with_sessions(vec![
            Session::new("session1"),
            Session::new("session2"),
        ]);
        state.selected_index = 0;
        state.focus_terminal();

        // Select a different session
        state.focus_sidebar();
        state.select_next();
        assert_eq!(state.selected_index, 1);

        // Jump back should return to previous session and focus terminal
        state.jump_back();
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_app_state_add_session() {
        let mut state = AppState::default();

        state.add_session(Session::new("new"));
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].name, "new");
        assert_eq!(state.selected_index, 0);

        state.add_session(Session::new("newer"));
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.sessions[0].name, "newer"); // Most recent at top
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_app_state_remove_session() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.selected_index = 2;

        assert!(state.remove_session(1));
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.sessions[0].name, "a");
        assert_eq!(state.sessions[1].name, "c");
        // Selection adjusted since we were at index 2
        assert_eq!(state.selected_index, 1);
    }

    #[test]
    fn test_app_state_remove_session_updates_previous() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.previous_session = Some(2);

        state.remove_session(1);
        // Previous was at 2, now should be at 1
        assert_eq!(state.previous_session, Some(1));

        state.previous_session = Some(0);
        state.remove_session(0);
        // Previous was the removed one, should be None
        assert!(state.previous_session.is_none());
    }

    #[test]
    fn test_app_state_remove_session_invalid_index() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        assert!(!state.remove_session(5)); // Invalid index
        assert_eq!(state.sessions.len(), 1);
    }

    #[test]
    fn test_app_state_rename_session() {
        let mut state = AppState::with_sessions(vec![Session::new("old")]);

        assert!(state.rename_session(0, "new".to_string()));
        assert_eq!(state.sessions[0].name, "new");

        assert!(!state.rename_session(5, "invalid".to_string()));
    }

    #[test]
    fn test_app_state_scroll_on_select_previous() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.selected_index = 2;
        state.scroll_offset = 2;

        state.select_previous();
        assert_eq!(state.selected_index, 1);
        // scroll_offset should adjust when selection goes above visible area
        assert_eq!(state.scroll_offset, 1);
    }

    #[test]
    fn test_cancel_create_mode() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::CreateMode { previous_focus: Focus::Terminal },
            ..Default::default()
        };

        state.cancel_create_mode();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_move_session_to_top() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.selected_index = 2;

        // Move session "c" (index 2) to top
        state.move_session_to_top(2);

        assert_eq!(state.sessions[0].name, "c");
        assert_eq!(state.sessions[1].name, "a");
        assert_eq!(state.sessions[2].name, "b");
        // Selection should follow the moved session
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_move_session_to_top_updates_previous() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.previous_session = Some(2);
        state.selected_index = 1;

        // Move session "c" (index 2, same as previous) to top
        state.move_session_to_top(2);

        // Previous should now point to 0 (where the session moved)
        assert_eq!(state.previous_session, Some(0));
    }

    #[test]
    fn test_move_session_to_top_index_0_is_noop() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
        ]);
        state.selected_index = 0;

        // Moving index 0 to top should be a no-op
        state.move_session_to_top(0);

        assert_eq!(state.sessions[0].name, "a");
        assert_eq!(state.sessions[1].name, "b");
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_move_selected_to_top() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
            Session::new("c"),
        ]);
        state.selected_index = 2;

        state.move_selected_to_top();

        assert_eq!(state.sessions[0].name, "c");
        assert_eq!(state.sessions[1].name, "a");
        assert_eq!(state.sessions[2].name, "b");
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_move_selected_to_top_already_at_top() {
        let mut state = AppState::with_sessions(vec![
            Session::new("a"),
            Session::new("b"),
        ]);
        state.selected_index = 0;

        state.move_selected_to_top();

        // Should be unchanged
        assert_eq!(state.sessions[0].name, "a");
        assert_eq!(state.sessions[1].name, "b");
        assert_eq!(state.selected_index, 0);
    }
}
