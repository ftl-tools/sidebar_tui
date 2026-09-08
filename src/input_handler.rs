//! Keyboard event handling with state machine integration.
//!
//! This module contains the input handler that routes key events based on the
//! current application mode and focus state.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{
    AppMode, AppState, ConfirmAction, ConfirmState, EventResult, Focus, RenamingState, WindowType,
    SessionOverlayMode,
};

impl AppState {
    /// Handle a key event based on current mode and focus.
    /// Returns EventResult indicating if the event was consumed, not consumed, or detach requested.
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
            AppMode::SessionOverlay(_) => {
                return self.handle_session_overlay_key(key);
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

    fn selected_window_event(&mut self, commit: bool) -> EventResult {
        let Some(name) = self
            .windows
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
            EventResult::SwitchWindow { name }
        } else {
            EventResult::PreviewWindow { name }
        }
    }

    /// Handle shortcuts that intentionally work from either normal focus region.
    fn handle_direct_shortcut(&mut self, key: KeyEvent) -> Option<EventResult> {
        if Self::is_toggle_key(&key) {
            return Some(if self.focus == Focus::Terminal {
                self.focus_sidebar();
                EventResult::Consumed
            } else {
                self.selected_window_event(true)
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
                if index < self.windows.len() {
                    self.selected_index = index;
                    self.selected_window_event(commit)
                } else {
                    EventResult::Consumed
                }
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.reorder_selected(-1);
                EventResult::ReorderWindow { offset: -1 }
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.reorder_selected(1);
                EventResult::ReorderWindow { offset: 1 }
            }
            KeyCode::Left | KeyCode::Right => {
                if key.code == KeyCode::Left {
                    if self.selected_index == 0 && !self.windows.is_empty() {
                        self.selected_index = self.windows.len() - 1;
                    } else {
                        self.select_previous();
                    }
                } else if !self.windows.is_empty() {
                    self.selected_index = (self.selected_index + 1) % self.windows.len();
                }
                self.selected_window_event(commit)
            }
            KeyCode::Up => EventResult::SwitchRelativeSession { offset: -1 },
            KeyCode::Down => EventResult::SwitchRelativeSession { offset: 1 },
            _ => return None,
        };
        if commit
            && self.selected_index != origin
            && matches!(result, EventResult::SwitchWindow { .. })
        {
            self.previous_window = Some(origin);
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
                KeyCode::Char('c') => return EventResult::OpenSessionCreate,
                KeyCode::Char('r') => return EventResult::OpenSessionRename,
                KeyCode::Char('k') => return EventResult::OpenSessionKill,
                KeyCode::Char('p') => return EventResult::SwitchRelativeSession { offset: -1 },
                KeyCode::Char('n') => return EventResult::SwitchRelativeSession { offset: 1 },
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
                self.selected_window_event(false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                self.selected_window_event(false)
            }
            KeyCode::Enter => self.selected_window_event(true),
            KeyCode::Esc | KeyCode::Char('q') => {
                self.cancel_browsing();
                self.selected_window_event(true)
            }
            KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                // The old two-step n→t prompt slowed the common path; c now drafts a terminal directly.
                self.start_drafting(WindowType::Terminal);
                EventResult::Consumed
            }
            KeyCode::Char('a') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.start_drafting(WindowType::Agent);
                EventResult::Consumed
            }
            KeyCode::Char('n') | KeyCode::Char('p') => {
                if !self.windows.is_empty() {
                    if key.code == KeyCode::Char('n') {
                        self.selected_index = (self.selected_index + 1) % self.windows.len();
                    } else if self.selected_index == 0 {
                        self.selected_index = self.windows.len() - 1;
                    } else {
                        self.selected_index -= 1;
                    }
                }
                self.selected_window_event(false)
            }
            KeyCode::Char('1'..='9') => {
                if let KeyCode::Char(c) = key.code {
                    let index = (c as u8 - b'1') as usize;
                    if index < self.windows.len() {
                        self.selected_index = index;
                    }
                }
                self.selected_window_event(false)
            }
            KeyCode::Char('l') => {
                self.jump_back();
                self.selected_window_event(true)
            }
            KeyCode::Char('r') | KeyCode::Char(',') => {
                if !self.windows.is_empty() {
                    self.start_renaming();
                }
                EventResult::Consumed
            }
            KeyCode::Char('&') | KeyCode::Delete => {
                if !self.windows.is_empty() {
                    self.request_confirmation(ConfirmAction::KillWindow(self.selected_index));
                }
                EventResult::Consumed
            }
            KeyCode::Char('m') => self
                .windows
                .get(self.selected_index)
                .map(|window| EventResult::OpenMoveToSessionOverlay {
                    window_name: window.name.clone(),
                })
                .unwrap_or(EventResult::Consumed),
            KeyCode::Char('s') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                EventResult::OpenSessionOverlay
            }
            KeyCode::Char('w') => EventResult::Consumed,
            KeyCode::Char('C') => EventResult::OpenSessionCreate,
            KeyCode::Char('R') | KeyCode::Char('$') => EventResult::OpenSessionRename,
            KeyCode::Char('K') => EventResult::OpenSessionKill,
            KeyCode::Char('P') => EventResult::SwitchRelativeSession { offset: -1 },
            KeyCode::Char('N') => EventResult::SwitchRelativeSession { offset: 1 },
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
                self.request_confirmation(ConfirmAction::Detach);
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

    /// Handle key events in create mode (selecting window type: t or a).
    /// Transitions into drafting mode so the user can type a custom window name.
    fn handle_create_mode_key(&mut self, key: KeyEvent) -> EventResult {
        match key.code {
            // Terminal window: enter drafting mode for user to type a name
            KeyCode::Char('t') => {
                self.start_drafting(WindowType::Terminal);
                EventResult::Consumed
            }
            // Agent window: enter drafting mode for user to type a name
            KeyCode::Char('a') => {
                self.start_drafting(WindowType::Agent);
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

    /// Handle key events while drafting a new window name.
    fn handle_drafting_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Drafting(ref mut draft) = self.mode {
            match key.code {
                // Create the window
                KeyCode::Enter => {
                    let window_type = draft.window_type;
                    // Empty names are now optional; generate the same collision-safe name used elsewhere.
                    let name = if draft.name.trim().is_empty() {
                        let existing: Vec<&str> =
                            self.windows.iter().map(|s| s.name.as_str()).collect();
                        crate::name_generator::generate_unique_window_name(&existing)
                    } else {
                        draft.name.trim().to_string()
                    };
                    self.mode = AppMode::Normal;
                    EventResult::CreateWindow { name, window_type }
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

    /// Handle key events while renaming a window.
    fn handle_renaming_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::Renaming(ref mut rename) = self.mode {
            match key.code {
                // Complete rename
                KeyCode::Enter => {
                    let index = rename.window_index;
                    let new_name = rename.new_name.clone();
                    // Get old name before updating local state
                    let old_name = self
                        .windows
                        .get(index)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if !new_name.is_empty() && !old_name.is_empty() {
                        // Update local state
                        self.rename_window(index, new_name.clone());
                        self.mode = AppMode::Normal;
                        // Sticky command mode keeps focus in the sidebar after editing.
                        self.focus = Focus::Sidebar;
                        // Return RenameWindow event for server to handle
                        return EventResult::RenameWindow { old_name, new_name };
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

    /// Handle key events while the session overlay is open.
    fn handle_session_overlay_key(&mut self, key: KeyEvent) -> EventResult {
        if let AppMode::SessionOverlay(ref mut overlay) = self.mode {
            // If we're in drafting mode (creating a new session), handle text input
            if overlay.drafting_session.is_some() {
                match key.code {
                    KeyCode::Enter => {
                        let name = overlay
                            .drafting_session
                            .as_ref()
                            .map(|d| d.new_name.trim().to_string())
                            .unwrap_or_default();
                        if name.is_empty() {
                            overlay.drafting_session = None;
                        } else {
                            let name_clone = name.clone();
                            overlay.drafting_session = None;
                            return EventResult::CreateSession { name: name_clone };
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Esc => {
                        overlay.drafting_session = None;
                        return EventResult::Consumed;
                    }
                    KeyCode::Char(c) => {
                        if let Some(ref mut draft) = overlay.drafting_session {
                            draft.insert_char(c);
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Backspace => {
                        if let Some(ref mut draft) = overlay.drafting_session {
                            draft.delete_char();
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Left => {
                        if let Some(ref mut draft) = overlay.drafting_session {
                            draft.move_cursor_left();
                        }
                        return EventResult::Consumed;
                    }
                    KeyCode::Right => {
                        if let Some(ref mut draft) = overlay.drafting_session {
                            draft.move_cursor_right();
                        }
                        return EventResult::Consumed;
                    }
                    _ => return EventResult::Consumed,
                }
            }

            // If we're renaming a session, handle text input
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
                                .sessions
                                .get(overlay.selected_index)
                                .cloned()
                                .unwrap_or_default();
                            if !old_name.is_empty() {
                                overlay.renaming = None;
                                return EventResult::RenameSession { old_name, new_name };
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
                    if let AppMode::SessionOverlay(ref mut ov) = self.mode {
                        ov.select_next();
                    }
                    return EventResult::Consumed;
                }
                // Navigate up
                KeyCode::Up | KeyCode::Char('k') => {
                    if let AppMode::SessionOverlay(ref mut ov) = self.mode {
                        ov.select_previous();
                    }
                    return EventResult::Consumed;
                }
                // Select (switch to session) or move window to session
                KeyCode::Enter => {
                    let (mode, selected, sessions, active_session) =
                        if let AppMode::SessionOverlay(ref ov) = self.mode {
                            (
                                ov.mode.clone(),
                                ov.selected_index,
                                ov.sessions.clone(),
                                ov.active_session.clone(),
                            )
                        } else {
                            return EventResult::Consumed;
                        };
                    let session_name = sessions.get(selected).cloned().unwrap_or_default();
                    self.mode = AppMode::Normal;
                    return match mode {
                        SessionOverlayMode::Normal => EventResult::SwitchSession {
                            name: session_name,
                        },
                        SessionOverlayMode::MoveWindow { window_name } => {
                            // Spec: "If the selected session is the current session, do nothing."
                            if session_name == active_session {
                                EventResult::Consumed
                            } else {
                                EventResult::MoveWindowToSession {
                                    window_name,
                                    session_name,
                                }
                            }
                        }
                    };
                }
                // Create/rename/delete are disabled in move mode
                KeyCode::Char('C') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::SessionOverlay(ref ov) if matches!(ov.mode, SessionOverlayMode::MoveWindow { .. })
                    );
                    if !is_move_mode {
                        if let AppMode::SessionOverlay(ref mut ov) = self.mode {
                            // Inline draft: scroll to top so draft row at virtual index 0 is visible
                            ov.selected_index = 0;
                            ov.scroll_offset = 0;
                            ov.drafting_session = Some(RenamingState::new(0, "", self.focus));
                        }
                    }
                    return EventResult::Consumed;
                }
                // Rename selected session (disabled in move mode)
                KeyCode::Char('R') | KeyCode::Char('$') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::SessionOverlay(ref ov) if matches!(ov.mode, SessionOverlayMode::MoveWindow { .. })
                    );
                    if !is_move_mode {
                        let selected_name = if let AppMode::SessionOverlay(ref ov) = self.mode {
                            ov.sessions
                                .get(ov.selected_index)
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        if !selected_name.is_empty() {
                            if let AppMode::SessionOverlay(ref mut ov) = self.mode {
                                ov.renaming =
                                    Some(RenamingState::new(0, &selected_name, Focus::Sidebar));
                            }
                        }
                    }
                    return EventResult::Consumed;
                }
                // Delete selected session (disabled in move mode) - shows confirmation
                KeyCode::Char('K') => {
                    let is_move_mode = matches!(
                        self.mode,
                        AppMode::SessionOverlay(ref ov) if matches!(ov.mode, SessionOverlayMode::MoveWindow { .. })
                    );
                    if !is_move_mode {
                        let (selected, sessions, active) =
                            if let AppMode::SessionOverlay(ref ov) = self.mode {
                                (
                                    ov.selected_index,
                                    ov.sessions.clone(),
                                    ov.active_session.clone(),
                                )
                            } else {
                                return EventResult::Consumed;
                            };
                        let session_name = sessions.get(selected).cloned().unwrap_or_default();
                        // Don't try to delete an empty session name
                        if session_name.is_empty() {
                            return EventResult::Consumed;
                        }
                        // Close overlay and show confirmation
                        self.mode = AppMode::Confirming(ConfirmState::new(
                            ConfirmAction::KillSession(session_name),
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
            // Check if 'q' should also confirm (only for Detach action)
            let is_detach_confirm =
                matches!(confirm.action, ConfirmAction::Detach) && key.code == KeyCode::Char('q');

            match key.code {
                // Confirm (yes, or 'q' for detach specifically)
                KeyCode::Char('y') => {
                    let action = confirm.action.clone();
                    self.mode = AppMode::Normal;
                    match action {
                        ConfirmAction::Detach => EventResult::Detach,
                        ConfirmAction::KillWindow(index) => {
                            // Get the window name before removing from local state
                            let name = self
                                .windows
                                .get(index)
                                .map(|s| s.name.clone())
                                .unwrap_or_default();
                            if !name.is_empty() {
                                // Remove from local state
                                self.remove_window(index);
                                // Return KillWindow event for server to handle
                                EventResult::KillWindow { name }
                            } else {
                                EventResult::Consumed
                            }
                        }
                        ConfirmAction::KillSession(name) => {
                            EventResult::KillSession { name }
                        }
                    }
                }
                // 'q' is alternative to 'y' for detach confirmation only
                KeyCode::Char('q') if is_detach_confirm => {
                    self.mode = AppMode::Normal;
                    EventResult::Detach
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
    use crate::state::{ConfirmState, DraftingState, RenamingState, Window};

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
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.focus = Focus::Sidebar;

        assert_eq!(state.selected_index, 0);

        let result = state.handle_key(key(KeyCode::Down));
        // Navigation returns PreviewWindow for live preview
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Down));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        // At bottom, should stay but still return preview for current selection
        let result = state.handle_key(key(KeyCode::Down));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        let result = state.handle_key(key(KeyCode::Up));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Up));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "a"));
        assert_eq!(state.selected_index, 0);

        // At top, should stay but still return preview for current selection
        let result = state.handle_key(key(KeyCode::Up));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "a"));
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_sidebar_vim_jk_navigation() {
        let mut state = AppState::with_windows(vec![
            Window::new("a"),
            Window::new("b"),
            Window::new("c"),
        ]);
        state.focus = Focus::Sidebar;

        assert_eq!(state.selected_index, 0);

        // j moves down (vim-style) and returns PreviewWindow
        let result = state.handle_key(key(KeyCode::Char('j')));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Char('j')));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        // At bottom, should stay but still return preview
        let result = state.handle_key(key(KeyCode::Char('j')));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "c"));
        assert_eq!(state.selected_index, 2);

        // k moves up (vim-style) and returns PreviewWindow
        let result = state.handle_key(key(KeyCode::Char('k')));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "b"));
        assert_eq!(state.selected_index, 1);

        let result = state.handle_key(key(KeyCode::Char('k')));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "a"));
        assert_eq!(state.selected_index, 0);

        // At top, should stay but still return preview
        let result = state.handle_key(key(KeyCode::Char('k')));
        assert!(matches!(result, EventResult::PreviewWindow { name } if name == "a"));
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_sidebar_enter_focuses_terminal() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Enter));
        // Now returns SwitchWindow instead of Consumed
        assert!(matches!(result, EventResult::SwitchWindow { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_space_focuses_terminal() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Char(' ')));
        // Now returns SwitchWindow instead of Consumed
        assert!(matches!(result, EventResult::SwitchWindow { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_right_focuses_terminal() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Right));
        // Now returns SwitchWindow instead of Consumed
        assert!(matches!(result, EventResult::SwitchWindow { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_tab_focuses_terminal() {
        let mut state = AppState::with_windows(vec![Window::new("test")]);
        state.focus = Focus::Sidebar;

        let result = state.handle_key(key(KeyCode::Tab));
        // Tab should focus terminal just like Enter, Space, and Right
        assert!(matches!(result, EventResult::SwitchWindow { name } if name == "test"));
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_enter_focuses_terminal_in_welcome_state() {
        // In welcome state (no windows), Enter should still focus the terminal so the
        // welcome text keybinding updates dynamically (shows ctrl+n instead of n).
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Enter));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.focus, Focus::Terminal); // Should switch to terminal even with no windows
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_esc_jump_back() {
        let mut state = AppState {
            windows: vec![Window::new("a"), Window::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_window to 0
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
            windows: vec![Window::new("a"), Window::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_window to 0
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
            windows: vec![Window::new("a"), Window::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_window to 0
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
            windows: vec![Window::new("a"), Window::new("b")],
            focus: Focus::Sidebar,
            selected_index: 0,
            ..Default::default()
        };
        state.focus_terminal(); // Sets previous_window to 0
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
            windows: vec![Window::new("test")],
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('d')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert!(matches!(confirm.action, ConfirmAction::KillWindow(0)));
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
            windows: vec![Window::new("old_name")],
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
    fn test_sidebar_q_requests_detach_confirmation() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Detach);
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
    fn test_terminal_ctrl_q_requests_detach_confirmation() {
        let mut state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('q'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Detach);
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_sidebar_ctrl_q_requests_detach_confirmation() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };

        let result = state.handle_key(ctrl_key('q'));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_)));
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(confirm.action, ConfirmAction::Detach);
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
                assert_eq!(draft.window_type, WindowType::Terminal);
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
                assert_eq!(draft.window_type, WindowType::Agent);
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
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
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
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
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
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
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
    fn test_drafting_enter_creates_window() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Char('t')));
        state.handle_key(key(KeyCode::Char('e')));
        state.handle_key(key(KeyCode::Char('s')));
        state.handle_key(key(KeyCode::Char('t')));

        let result = state.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, EventResult::CreateWindow { .. }));
        if let EventResult::CreateWindow { name, window_type } = result {
            assert_eq!(name, "test");
            assert_eq!(window_type, WindowType::Terminal);
        }
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_drafting_enter_with_empty_name_does_nothing() {
        let mut state = AppState {
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Sidebar)),
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
            mode: AppMode::Drafting(DraftingState::new(WindowType::Terminal, Focus::Terminal)),
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
            windows: vec![Window::new("old")],
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
        // Now returns RenameWindow instead of Consumed
        assert!(
            matches!(result, EventResult::RenameWindow { old_name, new_name }
            if old_name == "old" && new_name == "new")
        );
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.windows[0].name, "new");
        assert_eq!(state.focus, Focus::Terminal); // Per spec: rename confirm focuses terminal pane
    }

    #[test]
    fn test_renaming_esc_cancels() {
        let mut state = AppState {
            windows: vec![Window::new("original")],
            focus: Focus::Sidebar,
            mode: AppMode::Renaming(RenamingState::new(0, "original", Focus::Sidebar)),
            ..Default::default()
        };

        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Char('x')));

        let result = state.handle_key(key(KeyCode::Esc));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Normal));
        assert_eq!(state.windows[0].name, "original"); // Unchanged
        assert_eq!(state.focus, Focus::Sidebar); // Restored
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_renaming_enter_always_focuses_terminal() {
        // Per spec: "Exit rename mode and focus on the terminal pane" — always Terminal, regardless
        // of where focus was when renaming started.
        let mut state = AppState {
            windows: vec![Window::new("window")],
            focus: Focus::Terminal,
            mode: AppMode::Renaming(RenamingState::new(0, "window", Focus::Terminal)),
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
        assert!(matches!(result, EventResult::RenameWindow { .. }));
        assert_eq!(state.focus, Focus::Terminal); // Always focuses terminal after rename confirm
    }

    // === Confirmation Mode Tests ===

    #[test]
    fn test_confirm_detach_y_returns_detach() {
        let mut state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('y')));
        assert_eq!(result, EventResult::Detach);
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_confirm_delete_y_removes_window() {
        let mut state = AppState {
            windows: vec![Window::new("a"), Window::new("b")],
            mode: AppMode::Confirming(ConfirmState::new(
                ConfirmAction::KillWindow(0),
                Focus::Sidebar,
            )),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('y')));
        // Now returns KillWindow instead of Consumed
        assert!(matches!(result, EventResult::KillWindow { name } if name == "a"));
        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.windows[0].name, "b");
    }

    #[test]
    fn test_confirm_n_cancels() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
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
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Terminal)),
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
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('x')));
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_))); // Still confirming
    }

    #[test]
    fn test_confirm_detach_q_returns_detach() {
        let mut state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Detach, Focus::Sidebar)),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Detach);
        assert!(matches!(state.mode, AppMode::Normal));
    }

    #[test]
    fn test_confirm_delete_q_does_not_confirm() {
        let mut state = AppState {
            windows: vec![Window::new("test")],
            mode: AppMode::Confirming(ConfirmState::new(
                ConfirmAction::KillWindow(0),
                Focus::Sidebar,
            )),
            ..Default::default()
        };

        let result = state.handle_key(key(KeyCode::Char('q')));
        // 'q' should NOT confirm delete - only detach confirmation
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(state.mode, AppMode::Confirming(_))); // Still confirming
        assert_eq!(state.windows.len(), 1); // Window not deleted
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

    // === Session Overlay Tests ===

    fn session_overlay_state(
        sessions: Vec<&str>,
        active: &str,
    ) -> crate::state::SessionOverlayState {
        crate::state::SessionOverlayState::new(
            sessions.into_iter().map(|s| s.to_string()).collect(),
            active.to_string(),
        )
    }

    #[test]
    fn test_m_key_opens_move_to_session_overlay() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            windows: vec![Window::new("mywindow")],
            sessions: vec!["Default".to_string(), "Work".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('m')));
        assert!(
            matches!(result, EventResult::OpenMoveToSessionOverlay { ref window_name } if window_name == "mywindow"),
            "Expected OpenMoveToSessionOverlay, got {:?}",
            result
        );
    }

    #[test]
    fn test_m_key_does_nothing_with_no_windows() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            windows: vec![],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('m')));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_session_overlay_esc_closes() {
        let mut state = AppState {
            mode: AppMode::SessionOverlay(session_overlay_state(
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
    fn test_session_overlay_navigate_down() {
        let mut state = AppState {
            mode: AppMode::SessionOverlay(session_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Down));
        if let AppMode::SessionOverlay(ref ov) = state.mode {
            assert_eq!(ov.selected_index, 1);
        } else {
            panic!("Expected SessionOverlay mode");
        }
    }

    #[test]
    fn test_session_overlay_navigate_up() {
        let ov = {
            let mut ov = session_overlay_state(vec!["Default", "Work"], "Work");
            ov.selected_index = 1;
            ov
        };
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Up));
        if let AppMode::SessionOverlay(ref ov) = state.mode {
            assert_eq!(ov.selected_index, 0);
        } else {
            panic!("Expected SessionOverlay mode");
        }
    }

    #[test]
    fn test_session_overlay_enter_switches_session() {
        let mut state = AppState {
            mode: AppMode::SessionOverlay(session_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        // Navigate to "Work"
        state.handle_key(key(KeyCode::Down));
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::SwitchSession { ref name } if name == "Work"),
            "Expected SwitchSession(Work), got {:?}",
            result
        );
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_session_overlay_move_mode_enter_moves_window() {
        use crate::state::SessionOverlayState;
        let ov = {
            let mut ov = SessionOverlayState::new_move_mode(
                vec!["Default".to_string(), "Work".to_string()],
                "Default".to_string(),
                "mywindow".to_string(),
            );
            ov.selected_index = 1; // Select "Work"
            ov
        };
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::MoveWindowToSession { ref window_name, ref session_name }
                if window_name == "mywindow" && session_name == "Work"),
            "Expected MoveWindowToSession, got {:?}",
            result
        );
        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn test_session_overlay_move_mode_n_does_nothing() {
        use crate::state::SessionOverlayState;
        let ov = SessionOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(),
            "mywindow".to_string(),
        );
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
            ..Default::default()
        };
        // 'n' should be ignored in move mode
        let result = state.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, EventResult::Consumed);
        // No drafting_session should be set
        if let AppMode::SessionOverlay(ref ov) = state.mode {
            assert!(
                ov.drafting_session.is_none(),
                "drafting_session should not be set in move mode"
            );
        }
    }

    #[test]
    fn test_session_overlay_move_mode_d_does_nothing() {
        use crate::state::SessionOverlayState;
        let ov = SessionOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(),
            "mywindow".to_string(),
        );
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
            ..Default::default()
        };
        // 'd' should not delete in move mode
        state.handle_key(key(KeyCode::Down)); // select Work
        let result = state.handle_key(key(KeyCode::Char('d')));
        assert_eq!(result, EventResult::Consumed);
        // Mode should still be SessionOverlay (not Normal)
        assert!(matches!(state.mode, AppMode::SessionOverlay(_)));
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_session_overlay_normal_mode_n_creates_session() {
        let mut state = AppState {
            mode: AppMode::SessionOverlay(session_overlay_state(vec!["Default"], "Default")),
            ..Default::default()
        };
        state.handle_key(key(KeyCode::Char('n')));
        if let AppMode::SessionOverlay(ref ov) = state.mode {
            assert!(
                ov.drafting_session.is_some(),
                "drafting_session should be set after 'n'"
            );
        } else {
            panic!("Expected SessionOverlay mode");
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_ctrl_w_opens_session_overlay() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            sessions: vec!["Default".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(ctrl_key('w'));
        assert_eq!(result, EventResult::OpenSessionOverlay);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_w_opens_session_overlay_from_sidebar() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            sessions: vec!["Default".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('w')));
        assert_eq!(result, EventResult::OpenSessionOverlay);
    }

    #[test]
    fn test_w_does_not_open_session_overlay_from_terminal() {
        // bare 'w' should NOT open overlay from terminal (only ctrl+w)
        let mut state = AppState {
            focus: Focus::Terminal,
            sessions: vec!["Default".to_string()],
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('w')));
        // 'w' from terminal should be passed through, not consumed
        assert_eq!(result, EventResult::NotConsumed);
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_session_overlay_q_shows_detach_confirmation() {
        let mut state = AppState {
            mode: AppMode::SessionOverlay(session_overlay_state(
                vec!["Default", "Work"],
                "Default",
            )),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Char('q')));
        assert_eq!(result, EventResult::Consumed);
        // Overlay should be closed and detach confirmation should be shown
        assert!(
            matches!(state.mode, AppMode::Confirming(_)),
            "Mode should be Confirming after 'q'"
        );
        if let AppMode::Confirming(ref confirm) = state.mode {
            assert_eq!(
                confirm.action,
                ConfirmAction::Detach,
                "Should be Detach confirmation"
            );
        }
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_session_overlay_move_mode_q_shows_detach_confirmation() {
        use crate::state::SessionOverlayState;
        let ov = SessionOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(),
            "mywindow".to_string(),
        );
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
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
    fn test_move_to_same_session_is_noop() {
        // Spec: "If the selected session is the current session, do nothing."
        use crate::state::SessionOverlayState;
        let ov = SessionOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(), // active session
            "mywindow".to_string(),
        );
        // selected_index is 0, which is "Default" (same as active)
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Enter));
        // Should be Consumed (no-op), not MoveWindowToSession
        assert_eq!(
            result,
            EventResult::Consumed,
            "Moving to same session should be a no-op (Consumed), got {:?}",
            result
        );
        // Overlay should be closed
        assert!(
            matches!(state.mode, AppMode::Normal),
            "Overlay should close after no-op move"
        );
    }

    #[test]
    fn test_move_to_different_session_works() {
        use crate::state::SessionOverlayState;
        let mut ov = SessionOverlayState::new_move_mode(
            vec!["Default".to_string(), "Work".to_string()],
            "Default".to_string(), // active session
            "mywindow".to_string(),
        );
        // Select "Work" (index 1)
        ov.selected_index = 1;
        let mut state = AppState {
            mode: AppMode::SessionOverlay(ov),
            ..Default::default()
        };
        let result = state.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(result, EventResult::MoveWindowToSession {
                ref window_name, ref session_name
            } if window_name == "mywindow" && session_name == "Work"),
            "Moving to different session should emit MoveWindowToSession, got {:?}",
            result
        );
    }

    #[test]
    fn tmux_toggle_commits_and_ctrl_space_legacy_null_is_supported() {
        let mut state = AppState::with_windows(vec![Window::new("one"), Window::new("two")]);
        state.focus = Focus::Terminal;
        assert_eq!(state.handle_key(ctrl_key('b')), EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
        state.selected_index = 1;
        assert!(
            matches!(state.handle_key(key(KeyCode::Null)), EventResult::SwitchWindow { name } if name == "two")
        );
        assert_eq!(state.focus, Focus::Terminal);
    }

    #[test]
    fn tmux_sidebar_commands_create_browse_delete_and_detach() {
        let mut state = AppState::with_windows(vec![Window::new("one"), Window::new("two")]);
        state.focus = Focus::Sidebar;
        assert!(matches!(
            state.handle_key(key(KeyCode::Char('c'))),
            EventResult::Consumed
        ));
        assert!(matches!(state.mode, AppMode::Drafting(_)));
        state.mode = AppMode::Normal;
        assert!(
            matches!(state.handle_key(key(KeyCode::Char('n'))), EventResult::PreviewWindow { name } if name == "two")
        );
        assert_eq!(
            state.handle_key(key(KeyCode::Delete)),
            EventResult::Consumed
        );
        assert!(matches!(
            state.mode,
            AppMode::Confirming(ConfirmState {
                action: ConfirmAction::KillWindow(1),
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
                action: ConfirmAction::Detach,
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
        let mut state = AppState::with_windows(vec![Window::new("one"), Window::new("two")]);
        state.focus = Focus::Terminal;
        assert!(
            matches!(state.handle_key(modified_key(KeyCode::Char('2'), KeyModifiers::ALT)), EventResult::SwitchWindow { name } if name == "two")
        );
        assert_eq!(
            state.handle_key(modified_key(
                KeyCode::Left,
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )),
            EventResult::ReorderWindow { offset: -1 }
        );
        assert_eq!(state.windows[0].name, "two");
        assert_eq!(
            state.handle_key(modified_key(KeyCode::Down, KeyModifiers::ALT)),
            EventResult::SwitchRelativeSession { offset: 1 }
        );
    }

    #[test]
    fn modal_text_input_takes_precedence_over_toggle() {
        let mut state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };
        state.start_drafting(WindowType::Terminal);
        assert_eq!(state.handle_key(ctrl_key('b')), EventResult::Consumed);
        assert_eq!(state.focus, Focus::Sidebar);
        assert!(matches!(state.mode, AppMode::Drafting(_)));
    }
}
