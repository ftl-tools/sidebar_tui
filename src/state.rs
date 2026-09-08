//! Application state management for Sidebar TUI.
//!
//! This module defines the core state types for managing focus, modes, and windows
//! following patterns from gitui, Zellij, and ratatui examples.
//!
//! Old workspaces are now sessions and old sessions are windows, matching tmux.
//! Each window currently has one terminal pane; the sidebar is a UI region, not a pane.
//! Detach leaves windows running; killing a window or session terminates its processes.

/// Which region currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// Sidebar region is focused (window list).
    #[default]
    Sidebar,
    /// Terminal pane is focused.
    Terminal,
}

/// Type of window being created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowType {
    /// Standard terminal window.
    Terminal,
    /// Agent window (runs `claude` command on creation).
    Agent,
}

/// State for drafting a new window name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftingState {
    /// Type of window being created (Terminal or Agent).
    pub window_type: WindowType,
    /// Current draft name being typed.
    pub name: String,
    /// Cursor position within the name (byte index).
    pub cursor_position: usize,
    /// Focus before entering create mode (to restore on cancel).
    pub previous_focus: Focus,
}

impl DraftingState {
    /// Create a new DraftingState for the given window type.
    pub fn new(window_type: WindowType, previous_focus: Focus) -> Self {
        Self {
            window_type,
            name: String::new(),
            cursor_position: 0,
            previous_focus,
        }
    }

    /// Insert a character at the cursor position if it's a valid window name character.
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

/// State for renaming an existing window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamingState {
    /// Index of the window being renamed in the windows list.
    pub window_index: usize,
    /// New name being typed.
    pub new_name: String,
    /// Cursor position within the new name (byte index).
    pub cursor_position: usize,
    /// Focus before starting rename (to restore on cancel).
    pub previous_focus: Focus,
}

impl RenamingState {
    /// Create a new RenamingState from an existing window.
    pub fn new(window_index: usize, current_name: &str, previous_focus: Focus) -> Self {
        let cursor_position = current_name.len();
        Self {
            window_index,
            new_name: current_name.to_string(),
            cursor_position,
            previous_focus,
        }
    }

    /// Insert a character at the cursor position if it's a valid window name character.
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
    /// Kill a window by its index.
    KillWindow(usize),
    /// Kill a session (and all its windows) by name.
    KillSession(String),
    /// Detach the TUI.
    Detach,
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
            ConfirmAction::KillWindow(_) => "Kill this window permanently?",
            ConfirmAction::KillSession(_) => {
                "Kill session and ALL its windows permanently?"
            }
            ConfirmAction::Detach => "Detach Sidebar TUI?",
        }
    }

    /// Check if this confirmation represents an important destructive action.
    pub fn is_important(&self) -> bool {
        matches!(
            &self.action,
            ConfirmAction::KillWindow(_) | ConfirmAction::KillSession(_)
        )
    }
}

/// Mode for the session overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOverlayMode {
    /// Normal session management mode.
    Normal,
    /// Move-to-session mode (triggered by 'm' in sidebar).
    MoveWindow {
        /// Name of the window being moved.
        window_name: String,
    },
}

/// State for the session overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOverlayState {
    /// List of all sessions (sorted alphabetically).
    pub sessions: Vec<String>,
    /// Name of the currently active session.
    pub active_session: String,
    /// Index of the selected session in the list.
    pub selected_index: usize,
    /// Scroll offset for the session list.
    pub scroll_offset: usize,
    /// Last known visible height of the session list area (updated each render cycle).
    pub visible_height: usize,
    /// Mode of the overlay (normal or move-to-session).
    pub mode: SessionOverlayMode,
    /// If renaming: the new name being typed.
    pub renaming: Option<RenamingState>,
    /// If creating new session: the draft name being typed.
    pub drafting_session: Option<RenamingState>,
}

impl SessionOverlayState {
    /// Create a new overlay state in normal mode.
    pub fn new(sessions: Vec<String>, active_session: String) -> Self {
        let selected_index = sessions
            .iter()
            .position(|w| w == &active_session)
            .unwrap_or(0);
        Self {
            sessions,
            active_session,
            selected_index,
            scroll_offset: 0,
            visible_height: 20,
            mode: SessionOverlayMode::Normal,
            renaming: None,
            drafting_session: None,
        }
    }

    /// Create overlay state in move-to-session mode.
    pub fn new_move_mode(
        sessions: Vec<String>,
        active_session: String,
        window_name: String,
    ) -> Self {
        let selected_index = sessions
            .iter()
            .position(|w| w == &active_session)
            .unwrap_or(0);
        Self {
            sessions,
            active_session: active_session.clone(),
            selected_index,
            scroll_offset: 0,
            visible_height: 20,
            mode: SessionOverlayMode::MoveWindow { window_name },
            renaming: None,
            drafting_session: None,
        }
    }

    /// Move selection up.
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            }
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        let count = self.sessions.len()
            + if self.drafting_session.is_some() {
                1
            } else {
                0
            };
        if self.selected_index + 1 < count {
            self.selected_index += 1;
            // Scroll down if selection moved below visible area
            let visible_end = self.scroll_offset + self.visible_height;
            if self.selected_index >= visible_end {
                self.scroll_offset = self.selected_index + 1 - self.visible_height;
            }
        }
    }

    /// Get the selected session name (None if drafting new).
    pub fn selected_session(&self) -> Option<&str> {
        if let Some(drafting) = &self.drafting_session {
            // The draft row is at index 0
            if self.selected_index == 0 {
                return None;
            }
            let _ = drafting;
            self.sessions
                .get(self.selected_index.saturating_sub(1))
                .map(|s| s.as_str())
        } else {
            self.sessions.get(self.selected_index).map(|s| s.as_str())
        }
    }
}

/// Application mode - determines what input mode the TUI is in.
/// Modal states take precedence over focus-based input handling.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AppMode {
    /// Normal operation - input depends on current focus.
    #[default]
    Normal,
    /// Create mode - waiting for user to select window type (t or a).
    CreateMode {
        /// Focus before entering create mode (to restore on cancel).
        previous_focus: Focus,
    },
    /// Drafting a new window name.
    Drafting(DraftingState),
    /// Renaming an existing window.
    Renaming(RenamingState),
    /// Showing a confirmation prompt.
    Confirming(ConfirmState),
    /// Session overlay is open.
    SessionOverlay(SessionOverlayState),
    /// Full sidebar command reference opened with `?`.
    Help,
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

/// A terminal window in the sidebar list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// Window name displayed in the sidebar.
    pub name: String,
    /// Whether this window is currently attached (active in this TUI).
    pub is_attached: bool,
}

impl Window {
    /// Create a new Window with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_attached: false,
        }
    }

    /// Create a new attached Window.
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
    /// User requested to detach the TUI.
    Detach,
    /// Create a new window with the given name and type.
    CreateWindow {
        /// Name for the new window.
        name: String,
        /// Type of window (Terminal or Agent).
        window_type: WindowType,
    },
    /// Kill a window by name.
    KillWindow {
        /// Name of the window to kill.
        name: String,
    },
    /// Rename a window.
    RenameWindow {
        /// Old window name.
        old_name: String,
        /// New window name.
        new_name: String,
    },
    /// Switch to a different window by name.
    SwitchWindow {
        /// Name of the window to switch to.
        name: String,
    },
    /// Preview a window's terminal content (without fully attaching).
    /// Used when navigating the sidebar to show a live preview.
    PreviewWindow {
        /// Name of the window to preview.
        name: String,
    },
    /// Toggle mouse capture mode.
    /// When enabled: scroll wheel works but text selection is blocked.
    /// When disabled: native terminal text selection works.
    ToggleMouseMode,
    /// Toggle zoom mode: expands terminal pane to full width by hiding the sidebar.
    /// Allows clean text selection of terminal-only content in editors like VSCode.
    ToggleZoom,
    /// Switch to the previous/next session in display order.
    SwitchRelativeSession {
        offset: isize,
    },
    /// Reorder the selected window locally in display order.
    ReorderWindow {
        offset: isize,
    },
    /// Open session management and immediately start the requested action.
    OpenSessionCreate,
    OpenSessionRename,
    OpenSessionKill,
    /// Open session overlay in normal mode.
    OpenSessionOverlay,
    /// Open session overlay in move-to-session mode.
    OpenMoveToSessionOverlay {
        /// The window to move.
        window_name: String,
    },
    /// Switch to a different session.
    SwitchSession {
        /// Name of the session to switch to.
        name: String,
    },
    /// Create a new session.
    CreateSession {
        /// Name for the new session.
        name: String,
    },
    /// Rename a session.
    RenameSession {
        /// Old session name.
        old_name: String,
        /// New session name.
        new_name: String,
    },
    /// Kill a session and all its windows.
    KillSession {
        /// Name of the session to kill.
        name: String,
    },
    /// Move a window to a different session.
    MoveWindowToSession {
        /// Name of the window to move.
        window_name: String,
        /// Name of the destination session.
        session_name: String,
    },
}

/// Main application state.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Which region currently has focus.
    pub focus: Focus,
    /// Current application mode (Normal, CreateMode, Drafting, etc.).
    pub mode: AppMode,
    /// List of terminal windows (ordered by most recently used).
    pub windows: Vec<Window>,
    /// Index of the currently selected window in the sidebar.
    pub selected_index: usize,
    /// Scroll offset for the sidebar window list.
    pub scroll_offset: usize,
    /// Previously active window index for `l` (last window).
    pub previous_window: Option<usize>,
    /// Window that was active when sidebar browsing began; Esc/q restores it.
    pub browsing_origin: Option<usize>,
    /// Whether mouse capture is enabled (for scroll wheel support).
    /// When disabled, native terminal text selection works.
    pub mouse_mode: bool,
    /// Whether the terminal pane is zoomed to full width (sidebar hidden).
    /// Used to allow clean text selection of only terminal content in editors like VSCode.
    pub zoomed: bool,
    /// The name of the currently active session.
    pub session_name: String,
    /// List of all session names (for the session overlay).
    pub sessions: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focus: Focus::default(),
            mode: AppMode::default(),
            windows: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
            previous_window: None,
            browsing_origin: None,
            mouse_mode: true,
            zoomed: false,
            session_name: "Default".to_string(),
            sessions: vec!["Default".to_string()],
        }
    }
}

impl AppState {
    /// Create a new AppState with the given windows.
    pub fn with_windows(windows: Vec<Window>) -> Self {
        Self {
            windows,
            ..Default::default()
        }
    }

    /// Check if we're in the welcome state (no windows).
    pub fn is_welcome_state(&self) -> bool {
        self.windows.is_empty() && matches!(self.mode, AppMode::Normal)
    }

    /// Get the currently selected window, if any.
    pub fn selected_window(&self) -> Option<&Window> {
        self.windows.get(self.selected_index)
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
        if self.selected_index + 1 < self.windows.len() {
            self.selected_index += 1;
            // Note: scroll_offset adjustment will be handled during rendering
            // based on visible area height
        }
    }

    /// Commit the highlighted window and focus the terminal pane.
    pub fn focus_terminal(&mut self) {
        // The old implementation overwrote history with the selected index, making `l`
        // and browse cancellation unable to recover the window active before browsing.
        if let Some(origin) = self.browsing_origin.take() {
            if origin != self.selected_index {
                self.previous_window = Some(origin);
            }
        }
        self.focus = Focus::Terminal;
    }

    /// Focus on the sidebar region. Also exits zoom mode (sidebar was hidden while zoomed).
    pub fn focus_sidebar(&mut self) {
        // Snapshot once so live previews can be cancelled without committing selection.
        if self.focus == Focus::Terminal {
            self.browsing_origin = Some(self.selected_index);
        }
        self.focus = Focus::Sidebar;
        self.zoomed = false;
    }

    /// Enter create mode from current focus. Exits zoom mode so the sidebar is visible.
    pub fn enter_create_mode(&mut self) {
        self.zoomed = false;
        self.mode = AppMode::CreateMode {
            previous_focus: self.focus,
        };
    }

    /// Start drafting a new window with the given type.
    pub fn start_drafting(&mut self, window_type: WindowType) {
        let previous_focus = match &self.mode {
            AppMode::CreateMode { previous_focus } => *previous_focus,
            _ => self.focus,
        };
        self.mode = AppMode::Drafting(DraftingState::new(window_type, previous_focus));
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

    /// Start renaming the selected window.
    pub fn start_renaming(&mut self) {
        if let Some(window) = self.selected_window() {
            let state = RenamingState::new(self.selected_index, &window.name, self.focus);
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

    /// Cancel sidebar browsing and restore the window active when browsing began.
    pub fn cancel_browsing(&mut self) {
        if let Some(origin) = self.browsing_origin.take() {
            if origin < self.windows.len() {
                self.selected_index = origin;
            }
        }
        self.focus = Focus::Terminal;
    }

    /// Switch to the previously active window and focus it.
    pub fn jump_back(&mut self) {
        if let Some(previous) = self.previous_window {
            if previous < self.windows.len() {
                let current = self.selected_index;
                self.selected_index = previous;
                self.previous_window = Some(current);
            }
        }
        self.browsing_origin = None;
        self.focus = Focus::Terminal;
    }

    /// Move the highlighted window by one stable display position.
    pub fn reorder_selected(&mut self, offset: isize) {
        if self.windows.is_empty() {
            return;
        }
        let target = (self.selected_index as isize + offset)
            .clamp(0, self.windows.len().saturating_sub(1) as isize) as usize;
        if target != self.selected_index {
            self.windows.swap(self.selected_index, target);
            self.selected_index = target;
        }
    }

    /// Add a new window to the top of the list.
    pub fn add_window(&mut self, window: Window) {
        self.windows.insert(0, window);
        // Keep selection on the new window
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Remove a window by index. Returns true if removed.
    pub fn remove_window(&mut self, index: usize) -> bool {
        if index < self.windows.len() {
            self.windows.remove(index);
            // Adjust selection if necessary
            if self.selected_index >= self.windows.len() && !self.windows.is_empty() {
                self.selected_index = self.windows.len() - 1;
            }
            // Adjust previous_window
            if let Some(prev) = self.previous_window {
                use std::cmp::Ordering;
                match prev.cmp(&index) {
                    Ordering::Equal => self.previous_window = None,
                    Ordering::Greater => self.previous_window = Some(prev - 1),
                    Ordering::Less => {}
                }
            }
            true
        } else {
            false
        }
    }

    /// Rename a window by index. Returns true if renamed.
    pub fn rename_window(&mut self, index: usize, new_name: String) -> bool {
        if let Some(window) = self.windows.get_mut(index) {
            window.name = new_name;
            true
        } else {
            false
        }
    }

    /// Move a window to the top of the list (most recently used).
    /// Used when a window becomes active (e.g., user sends input).
    pub fn move_window_to_top(&mut self, index: usize) {
        if index > 0 && index < self.windows.len() {
            let window = self.windows.remove(index);
            self.windows.insert(0, window);
            // Adjust selected_index to keep selection on the same window
            if self.selected_index == index {
                self.selected_index = 0;
            } else if self.selected_index < index {
                // Window was below selection, no adjustment needed
            } else {
                // This shouldn't happen since we only move windows above the selection
            }
            // Adjust previous_window
            if let Some(prev) = self.previous_window {
                if prev == index {
                    self.previous_window = Some(0);
                } else if prev < index {
                    self.previous_window = Some(prev + 1);
                }
            }
            // Adjust scroll_offset if needed
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.min(self.selected_index);
            }
        }
    }

    /// Move the currently selected window to the top of the list.
    pub fn move_selected_to_top(&mut self) {
        if self.selected_index > 0 && !self.windows.is_empty() {
            let window = self.windows.remove(self.selected_index);
            self.windows.insert(0, window);
            // Adjust previous_window
            if let Some(prev) = self.previous_window {
                if prev == self.selected_index {
                    self.previous_window = Some(0);
                } else if prev < self.selected_index {
                    self.previous_window = Some(prev + 1);
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

    // WindowType tests
    #[test]
    fn test_window_type_equality() {
        assert_eq!(WindowType::Terminal, WindowType::Terminal);
        assert_eq!(WindowType::Agent, WindowType::Agent);
        assert_ne!(WindowType::Terminal, WindowType::Agent);
    }

    // DraftingState tests
    #[test]
    fn test_drafting_state_new() {
        let state = DraftingState::new(WindowType::Terminal, Focus::Terminal);
        assert_eq!(state.window_type, WindowType::Terminal);
        assert_eq!(state.name, "");
        assert_eq!(state.cursor_position, 0);
        assert_eq!(state.previous_focus, Focus::Terminal);
    }

    #[test]
    fn test_drafting_state_insert_valid_chars() {
        let mut state = DraftingState::new(WindowType::Terminal, Focus::Sidebar);
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
        let mut state = DraftingState::new(WindowType::Terminal, Focus::Sidebar);
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
        let mut state = DraftingState::new(WindowType::Terminal, Focus::Sidebar);
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
        let mut state = DraftingState::new(WindowType::Terminal, Focus::Sidebar);
        state.delete_char();
        assert_eq!(state.name, "");
        assert_eq!(state.cursor_position, 0);
    }

    #[test]
    fn test_drafting_state_cursor_movement() {
        let mut state = DraftingState::new(WindowType::Terminal, Focus::Sidebar);
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
        assert_eq!(state.window_index, 2);
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
        assert_eq!(ConfirmAction::Detach, ConfirmAction::Detach);
        assert_eq!(
            ConfirmAction::KillWindow(0),
            ConfirmAction::KillWindow(0)
        );
        assert_ne!(
            ConfirmAction::KillWindow(0),
            ConfirmAction::KillWindow(1)
        );
        assert_ne!(ConfirmAction::Detach, ConfirmAction::KillWindow(0));
    }

    // ConfirmState tests
    #[test]
    fn test_confirm_state_message_detach() {
        let state = ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar);
        assert_eq!(state.message(), "Detach Sidebar TUI?");
    }

    #[test]
    fn test_confirm_state_message_delete() {
        let state = ConfirmState::new(ConfirmAction::KillWindow(0), Focus::Sidebar);
        assert_eq!(state.message(), "Kill this window permanently?");
    }

    #[test]
    fn test_confirm_state_is_important() {
        let detach_state = ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar);
        assert!(!detach_state.is_important());

        let delete_state = ConfirmState::new(ConfirmAction::KillWindow(0), Focus::Sidebar);
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
        assert!(
            !AppMode::CreateMode {
                previous_focus: Focus::Sidebar
            }
            .is_text_input()
        );
        assert!(
            AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar))
                .is_text_input()
        );
        assert!(AppMode::Renaming(RenamingState::new(0, "test", Focus::Sidebar)).is_text_input());
        assert!(
            !AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar))
                .is_text_input()
        );
    }

    #[test]
    fn test_app_mode_is_modal() {
        assert!(!AppMode::Normal.is_modal());
        assert!(
            AppMode::CreateMode {
                previous_focus: Focus::Sidebar
            }
            .is_modal()
        );
        assert!(
            AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)).is_modal()
        );
        assert!(AppMode::Renaming(RenamingState::new(0, "test", Focus::Sidebar)).is_modal());
        assert!(
            AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)).is_modal()
        );
    }

    // Window tests
    #[test]
    fn test_window_new() {
        let window = Window::new("my-window");
        assert_eq!(window.name, "my-window");
        assert!(!window.is_attached);
    }

    #[test]
    fn test_window_attached() {
        let window = Window::attached("main");
        assert_eq!(window.name, "main");
        assert!(window.is_attached);
    }

    // EventResult tests
    #[test]
    fn test_event_result_equality() {
        assert_eq!(EventResult::Consumed, EventResult::Consumed);
        assert_eq!(EventResult::NotConsumed, EventResult::NotConsumed);
        assert_eq!(EventResult::Detach, EventResult::Detach);
        assert_ne!(EventResult::Consumed, EventResult::NotConsumed);
    }

    // AppState tests
    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert_eq!(state.focus, Focus::Sidebar);
        assert_eq!(state.mode, AppMode::Normal);
        assert!(state.windows.is_empty());
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.scroll_offset, 0);
        assert!(state.previous_window.is_none());
    }

    #[test]
    fn test_app_state_with_windows() {
        let windows = vec![Window::new("window1"), Window::new("window2")];
        let state = AppState::with_windows(windows);
        assert_eq!(state.windows.len(), 2);
        assert_eq!(state.windows[0].name, "window1");
    }

    #[test]
    fn test_app_state_is_welcome_state() {
        let empty_state = AppState::default();
        assert!(empty_state.is_welcome_state());

        let with_windows = AppState::with_windows(vec![Window::new("test")]);
        assert!(!with_windows.is_welcome_state());

        let modal_state = AppState {
            mode: AppMode::CreateMode {
                previous_focus: Focus::Sidebar,
            },
            ..Default::default()
        };
        assert!(!modal_state.is_welcome_state());
    }

    #[test]
    fn test_app_state_selected_window() {
        let state = AppState::default();
        assert!(state.selected_window().is_none());

        let state = AppState::with_windows(vec![Window::new("test")]);
        assert_eq!(state.selected_window().unwrap().name, "test");
    }

    #[test]
    fn test_app_state_select_navigation() {
        let mut state = AppState::with_windows(vec![
            Window::new("window1"),
            Window::new("window2"),
            Window::new("window3"),
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
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_app_state_focus_terminal() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.selected_index = 0;

        state.focus_terminal();
        assert_eq!(state.focus, Focus::Terminal);
        assert_eq!(state.previous_window, Some(0));
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
            mode: AppMode::CreateMode {
                previous_focus: Focus::Terminal,
            },
            ..Default::default()
        };

        state.start_drafting(WindowType::Agent);
        match &state.mode {
            AppMode::Drafting(draft) => {
                assert_eq!(draft.window_type, WindowType::Agent);
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
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Terminal)),
            ..Default::default()
        };

        state.cancel_drafting();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_app_state_start_renaming() {
        let mut state = AppState::with_windows(vec![Window::new("old_name")]);
        state.focus = Focus::Sidebar;

        state.start_renaming();
        match &state.mode {
            AppMode::Renaming(rename) => {
                assert_eq!(rename.window_index, 0);
                assert_eq!(rename.new_name, "old_name");
                assert_eq!(rename.previous_focus, Focus::Sidebar);
            }
            _ => panic!("Expected Renaming"),
        }
    }

    #[test]
    fn test_app_state_cancel_renaming() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
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

        state.request_confirmation(ConfirmAction::Detach);
        match &state.mode {
            AppMode::Confirming(confirm) => {
                assert_eq!(confirm.action, ConfirmAction::Detach);
                assert_eq!(confirm.previous_focus, Focus::Sidebar);
            }
            _ => panic!("Expected Confirming"),
        }
    }

    #[test]
    fn test_app_state_cancel_confirmation() {
        let mut state = AppState {
            focus: Focus::Terminal,
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Terminal)),
            ..Default::default()
        };

        state.cancel_confirmation();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_app_state_jump_back() {
        let mut state =
            AppState::with_windows(vec![Window::new("window1"), Window::new("window2")]);
        state.selected_index = 0;
        state.focus_terminal();

        // Select a different window
        state.focus_sidebar();
        state.select_next();
        assert_eq!(state.selected_index, 1);

        // Jump back should return to previous window and focus terminal
        state.jump_back();
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_app_state_add_window() {
        let mut state = AppState::default();

        state.add_window(Window::new("new"));
        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.windows[0].name, "new");
        assert_eq!(state.selected_index, 0);

        state.add_window(Window::new("newer"));
        assert_eq!(state.windows.len(), 2);
        assert_eq!(state.windows[0].name, "newer"); // Most recent at top
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_app_state_remove_window() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.selected_index = 2;

        assert!(state.remove_window(1));
        assert_eq!(state.windows.len(), 2);
        assert_eq!(state.windows[0].name, "a");
        assert_eq!(state.windows[1].name, "c");
        // Selection adjusted since we were at index 2
        assert_eq!(state.selected_index, 1);
    }

    #[test]
    fn test_app_state_remove_window_updates_previous() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.previous_window = Some(2);

        state.remove_window(1);
        // Previous was at 2, now should be at 1
        assert_eq!(state.previous_window, Some(1));

        state.previous_window = Some(0);
        state.remove_window(0);
        // Previous was the removed one, should be None
        assert!(state.previous_window.is_none());
    }

    #[test]
    fn test_app_state_remove_window_invalid_index() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
        assert!(!state.remove_window(5)); // Invalid index
        assert_eq!(state.windows.len(), 1);
    }

    #[test]
    fn test_app_state_rename_window() {
        let mut state = AppState::with_windows(vec![Window::new("old")]);

        assert!(state.rename_window(0, "new".to_string()));
        assert_eq!(state.windows[0].name, "new");

        assert!(!state.rename_window(5, "invalid".to_string()));
    }

    #[test]
    fn test_app_state_scroll_on_select_previous() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
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
            mode: AppMode::CreateMode {
                previous_focus: Focus::Terminal,
            },
            ..Default::default()
        };

        state.cancel_create_mode();
        assert_eq!(state.mode, AppMode::Normal);
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn test_move_window_to_top() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.selected_index = 2;

        // Move window "c" (index 2) to top
        state.move_window_to_top(2);

        assert_eq!(state.windows[0].name, "c");
        assert_eq!(state.windows[1].name, "a");
        assert_eq!(state.windows[2].name, "b");
        // Selection should follow the moved window
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_move_window_to_top_updates_previous() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.previous_window = Some(2);
        state.selected_index = 1;

        // Move window "c" (index 2, same as previous) to top
        state.move_window_to_top(2);

        // Previous should now point to 0 (where the window moved)
        assert_eq!(state.previous_window, Some(0));
    }

    #[test]
    fn test_move_window_to_top_index_0_is_noop() {
        let mut state = AppState::with_windows(vec![Window::new("a"), Window::new("b")]);
        state.selected_index = 0;

        // Moving index 0 to top should be a no-op
        state.move_window_to_top(0);

        assert_eq!(state.windows[0].name, "a");
        assert_eq!(state.windows[1].name, "b");
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_move_selected_to_top() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.selected_index = 2;

        state.move_selected_to_top();

        assert_eq!(state.windows[0].name, "c");
        assert_eq!(state.windows[1].name, "a");
        assert_eq!(state.windows[2].name, "b");
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_move_selected_to_top_already_at_top() {
        let mut state = AppState::with_windows(vec![Window::new("a"), Window::new("b")]);
        state.selected_index = 0;

        state.move_selected_to_top();

        // Should be unchanged
        assert_eq!(state.windows[0].name, "a");
        assert_eq!(state.windows[1].name, "b");
        assert_eq!(state.selected_index, 0);
    }

    // SessionOverlayState scroll tests

    #[test]
    fn test_session_overlay_select_next_scrolls_down() {
        let sessions = vec!["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut ov = SessionOverlayState::new(sessions, "a".to_string());
        ov.visible_height = 3; // Only 3 rows visible at a time
        assert_eq!(ov.selected_index, 0);
        assert_eq!(ov.scroll_offset, 0);

        // Navigate within visible area — no scroll needed
        ov.select_next();
        assert_eq!(ov.selected_index, 1);
        assert_eq!(ov.scroll_offset, 0);

        ov.select_next();
        assert_eq!(ov.selected_index, 2);
        assert_eq!(ov.scroll_offset, 0);

        // Moving to index 3 goes beyond visible_end (0 + 3 = 3) — should scroll
        ov.select_next();
        assert_eq!(ov.selected_index, 3);
        assert_eq!(ov.scroll_offset, 1); // 3 + 1 - 3 = 1
    }

    #[test]
    fn test_session_overlay_select_next_does_not_overflow() {
        let sessions = vec!["a", "b", "c"].into_iter().map(String::from).collect();
        let mut ov = SessionOverlayState::new(sessions, "a".to_string());
        ov.visible_height = 10;

        ov.select_next(); // index 1
        ov.select_next(); // index 2
        ov.select_next(); // at end — no change
        assert_eq!(ov.selected_index, 2);
        assert_eq!(ov.scroll_offset, 0);
    }

    #[test]
    fn test_session_overlay_select_previous_scrolls_up() {
        let sessions = vec!["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut ov = SessionOverlayState::new(sessions, "a".to_string());
        ov.visible_height = 3;
        ov.selected_index = 3;
        ov.scroll_offset = 1;

        // Move up within visible area — no scroll needed
        ov.select_previous();
        assert_eq!(ov.selected_index, 2);
        assert_eq!(ov.scroll_offset, 1);

        // Moving to index 1 is below scroll_offset (1) — no change needed
        ov.select_previous();
        assert_eq!(ov.selected_index, 1);
        assert_eq!(ov.scroll_offset, 1);

        // Moving to index 0 goes above scroll_offset — should scroll up
        ov.select_previous();
        assert_eq!(ov.selected_index, 0);
        assert_eq!(ov.scroll_offset, 0);
    }
}
