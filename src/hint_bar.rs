//! Hint bar module for displaying context-aware keybindings.
//!
//! The hint column lives inside the sidebar below the session list and shows:
//! - Available keybindings based on current context
//! - Confirmation prompts without obscuring the terminal's background
//! - Temporary messages
//! - Quit path below the contextual bindings

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};

use crate::colors;
use crate::state::{AppMode, AppState, ConfirmAction, Focus};

/// Information about a single keybinding to display in the hint bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingInfo {
    /// The key label (e.g., "ctrl + n", "q", "enter").
    pub key: String,
    /// Description of the action (e.g., "New", "Quit", "Select").
    pub description: String,
    /// Whether this binding is currently enabled (disabled bindings are grayed out).
    pub enabled: bool,
}

impl KeybindingInfo {
    /// Create a new enabled KeybindingInfo.
    pub fn new(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            enabled: true,
        }
    }

    /// Mark this keybinding as disabled (will be grayed out).
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Calculate the display width of this keybinding entry.
    /// Format: "key description" (key + space + description).
    pub fn display_width(&self) -> usize {
        self.key.len() + 1 + self.description.len()
    }
}

/// Mode of the hint bar display.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum HintBarMode {
    /// Normal keybinding display.
    #[default]
    Normal,
    /// Confirmation prompt with message.
    Confirm {
        /// The prompt message to display.
        message: String,
        /// Whether the action is important/destructive.
        important: bool,
    },
    /// Temporary message (replaces keybindings briefly).
    Message {
        /// The message text to display.
        text: String,
    },
}

/// Hint bar widget for rendering inside the sidebar.
#[derive(Debug, Clone)]
pub struct HintBar {
    /// Currently displayed keybindings.
    pub bindings: Vec<KeybindingInfo>,
    /// Current display mode.
    pub mode: HintBarMode,
    /// Path to quit shown below the bindings (e.g., "ctrl + b → q Quit").
    pub quit_path: String,
}

impl Default for HintBar {
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
            mode: HintBarMode::Normal,
            quit_path: String::new(),
        }
    }
}

impl HintBar {
    /// Create a new HintBar with the given bindings and quit path.
    pub fn new(bindings: Vec<KeybindingInfo>, quit_path: impl Into<String>) -> Self {
        Self {
            bindings,
            mode: HintBarMode::Normal,
            quit_path: quit_path.into(),
        }
    }

    /// Set the hint bar mode.
    pub fn with_mode(mut self, mode: HintBarMode) -> Self {
        self.mode = mode;
        self
    }

    /// Show a confirmation prompt.
    pub fn show_confirm(&mut self, message: &str, bindings: Vec<KeybindingInfo>, important: bool) {
        self.mode = HintBarMode::Confirm {
            message: message.to_string(),
            important,
        };
        self.bindings = bindings;
    }

    /// Show a temporary message.
    pub fn show_message(&mut self, text: &str) {
        self.mode = HintBarMode::Message {
            text: text.to_string(),
        };
    }

    /// Reset to normal mode with the given bindings.
    pub fn set_bindings(&mut self, bindings: Vec<KeybindingInfo>) {
        self.mode = HintBarMode::Normal;
        self.bindings = bindings;
    }

    /// Set the quit path.
    pub fn set_quit_path(&mut self, quit_path: impl Into<String>) {
        self.quit_path = quit_path.into();
    }

    /// Calculate the height needed by the wrapped sidebar column.
    pub fn calculate_height(&self, width: u16) -> u16 {
        self.column()
            .line_count(width.max(1))
            .min(u16::MAX as usize) as u16
    }

    // Horizontal packing resized the PTY whenever hints changed. A column keeps
    // contextual UI inside the sidebar, and Paragraph handles Unicode wrapping.
    fn column(&self) -> ratatui::widgets::Paragraph<'static> {
        let mut lines = Vec::new();
        match &self.mode {
            HintBarMode::Confirm { message, .. } => lines.push(Line::from(message.clone())),
            HintBarMode::Message { text } => lines.push(Line::from(text.clone())),
            HintBarMode::Normal => {}
        }
        if !matches!(self.mode, HintBarMode::Message { .. }) {
            for binding in &self.bindings {
                // The pinned exit path already displays this action; do not repeat it.
                if format!("{} {}", binding.key, binding.description) == self.quit_path {
                    continue;
                }
                lines.push(Line::from(vec![
                    Span::styled(
                        binding.key.clone(),
                        Style::default().fg(if binding.enabled {
                            colors::PURPLE
                        } else {
                            colors::DARK_GREY
                        }),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        binding.description.clone(),
                        Style::default().fg(if binding.enabled {
                            colors::WHITE
                        } else {
                            colors::DARK_GREY
                        }),
                    ),
                ]));
            }
        }
        if !self.quit_path.is_empty() {
            lines.push(Line::from(
                color_quit_path(&self.quit_path)
                    .into_iter()
                    .map(|(text, color)| Span::styled(text.to_owned(), Style::default().fg(color)))
                    .collect::<Vec<_>>(),
            ));
        }
        // The old solid grey/red fill made the hints look like labels. Preserve
        // only foreground emphasis so hint text sits directly on the terminal background.
        ratatui::widgets::Paragraph::new(lines)
            .style(Style::default().fg(colors::WHITE))
            .wrap(ratatui::widgets::Wrap { trim: false })
    }
}

impl Widget for HintBar {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        // Unlike the old right-aligned footer, reserve the last rows for the
        // escape path even when a short sidebar clips the contextual bindings.
        self.bindings
            .retain(|binding| format!("{} {}", binding.key, binding.description) != self.quit_path);
        let quit = HintBar {
            bindings: vec![],
            mode: HintBarMode::Normal,
            quit_path: std::mem::take(&mut self.quit_path),
        };
        let quit_height = if quit.quit_path.is_empty() {
            0
        } else {
            quit.calculate_height(area.width).min(area.height)
        };
        let content_area = Rect {
            height: area.height - quit_height,
            ..area
        };
        self.column().render(content_area, buf);
        quit.column()
            .style(Style::default().fg(colors::WHITE))
            .render(
                Rect::new(area.x, content_area.bottom(), area.width, quit_height),
                buf,
            );
    }
}

/// Parse a quit path string and return colored segments.
/// Keys are colored purple, arrows and "Quit" are colored white.
/// Example: "ctrl + b → q Quit" -> [("ctrl + b", PURPLE), (" → ", WHITE), ("q", PURPLE), (" ", WHITE), ("Quit", WHITE)]
fn color_quit_path(quit_path: &str) -> Vec<(&str, ratatui::style::Color)> {
    use ratatui::style::Color;

    let mut result = Vec::new();

    // Check if it ends with " Quit"
    if let Some(prefix) = quit_path.strip_suffix(" Quit") {
        // Split by " → " to find key segments
        let parts: Vec<&str> = prefix.split(" → ").collect();

        for (i, part) in parts.iter().enumerate() {
            // Each part is a key - color it purple
            result.push((*part, colors::PURPLE));

            // Add separator " → " if not the last part
            if i < parts.len() - 1 {
                // Find where " → " appears after this part
                let start = quit_path.find(part).unwrap() + part.len();
                let arrow_slice = &quit_path[start..start + 5]; // " → " is 5 bytes (space + 3-byte arrow + space)
                result.push((arrow_slice, colors::WHITE));
            }
        }

        // Add " Quit" at the end
        let quit_start = quit_path.len() - 5; // " Quit" is 5 chars
        result.push((&quit_path[quit_start..quit_start + 1], Color::Reset)); // space
        result.push((&quit_path[quit_start + 1..], colors::WHITE)); // "Quit"
    } else {
        // Fallback: just render everything white
        result.push((quit_path, colors::WHITE));
    }

    result
}

/// Get the keybindings for the current app state.
pub fn get_bindings_for_state(state: &AppState) -> Vec<KeybindingInfo> {
    // Mouse mode indicator - shows current state and how to toggle
    let mouse_desc = if state.mouse_mode {
        "Mouse scroll"
    } else {
        "Text select"
    };

    match &state.mode {
        AppMode::Normal => match state.focus {
            Focus::Sidebar => {
                if state.is_welcome_state() {
                    vec![
                        KeybindingInfo::new("c/a", "New terminal/agent"),
                        KeybindingInfo::new("s", "Workspaces"),
                        KeybindingInfo::new("S", mouse_desc),
                        KeybindingInfo::new("?", "Help"),
                        KeybindingInfo::new("d", "Detach"),
                    ]
                } else {
                    vec![
                        KeybindingInfo::new("enter/toggle", "Select"),
                        KeybindingInfo::new("esc/q", "Cancel"),
                        KeybindingInfo::new("↑/↓/j/k", "Browse"),
                        KeybindingInfo::new("1-9", "Highlight"),
                        KeybindingInfo::new("n/p/l", "Next/prev/last"),
                        KeybindingInfo::new("c/a", "New terminal/agent"),
                        KeybindingInfo::new("r/,", "Rename"),
                        KeybindingInfo::new("&/delete", "Delete"),
                        KeybindingInfo::new("m", "Move"),
                        KeybindingInfo::new("s", "Workspaces"),
                        KeybindingInfo::new("z", "Hide"),
                        KeybindingInfo::new("S", mouse_desc),
                        KeybindingInfo::new("?", "Help"),
                        KeybindingInfo::new("d", "Detach"),
                    ]
                }
            }
            Focus::Terminal => {
                vec![
                    KeybindingInfo::new("ctrl + space/b", "Sidebar"),
                    KeybindingInfo::new("alt + 1-9/←/→", "Switch window"),
                    KeybindingInfo::new("alt + ↑/↓", "Switch workspace"),
                ]
            }
        },
        AppMode::CreateMode { .. } => vec![
            KeybindingInfo::new("t", "Terminal Session"),
            KeybindingInfo::new("a", "Agent Session"),
            KeybindingInfo::new("esc", "Cancel"),
        ],
        AppMode::Drafting(_) => vec![
            KeybindingInfo::new("enter", "Create"),
            KeybindingInfo::new("esc", "Cancel"),
        ],
        AppMode::Renaming(_) => vec![
            KeybindingInfo::new("enter", "Rename"),
            KeybindingInfo::new("esc", "Cancel"),
        ],
        AppMode::Confirming(confirm_state) => {
            // Show "y/q" for quit confirmation, just "y" for others
            let yes_key = if matches!(confirm_state.action, ConfirmAction::Quit) {
                "y/q"
            } else {
                "y"
            };
            vec![
                KeybindingInfo::new(yes_key, "Yes"),
                KeybindingInfo::new("n", "No"),
            ]
        }
        AppMode::WorkspaceOverlay(overlay) => {
            use crate::state::WorkspaceOverlayMode;
            if overlay.drafting_workspace.is_some() {
                vec![
                    KeybindingInfo::new("enter", "Create"),
                    KeybindingInfo::new("esc", "Cancel"),
                ]
            } else if overlay.renaming.is_some() {
                vec![
                    KeybindingInfo::new("enter", "Rename"),
                    KeybindingInfo::new("esc", "Cancel"),
                ]
            } else if matches!(overlay.mode, WorkspaceOverlayMode::MoveSession { .. }) {
                vec![
                    KeybindingInfo::new("enter", "Move here"),
                    KeybindingInfo::new("↑/↓/j/k", "Navigate"),
                    KeybindingInfo::new("esc", "Cancel"),
                    KeybindingInfo::new("q", "Quit"),
                ]
            } else {
                vec![
                    KeybindingInfo::new("enter", "Switch"),
                    KeybindingInfo::new("↑/↓/j/k", "Navigate"),
                    KeybindingInfo::new("C", "New"),
                    KeybindingInfo::new("R/$", "Rename"),
                    KeybindingInfo::new("K", "Delete"),
                    KeybindingInfo::new("esc/q", "Close"),
                ]
            }
        }
        AppMode::Help => vec![KeybindingInfo::new("esc/q/?", "Close")],
    }
}

/// Get the quit path string for the current app state.
pub fn get_quit_path_for_state(state: &AppState) -> String {
    match &state.mode {
        AppMode::Normal => match state.focus {
            Focus::Sidebar => "d Detach".to_string(),
            Focus::Terminal => "toggle → d Detach".to_string(),
        },
        AppMode::CreateMode { .. } => "esc → q Quit".to_string(),
        AppMode::Drafting(_) | AppMode::Renaming(_) => "esc → q Quit".to_string(),
        AppMode::Confirming(_) => "n → q Quit".to_string(),
        AppMode::WorkspaceOverlay(_) => "esc/q Close".to_string(),
        AppMode::Help => "esc/q Close".to_string(),
    }
}

/// Create a HintBar configured for the current app state.
pub fn hint_bar_for_state(state: &AppState) -> HintBar {
    let bindings = get_bindings_for_state(state);
    let quit_path = get_quit_path_for_state(state);

    let mut hint_bar = HintBar::new(bindings, quit_path);

    // If in confirmation mode, show the confirmation prompt
    if let AppMode::Confirming(confirm_state) = &state.mode {
        hint_bar.mode = HintBarMode::Confirm {
            message: confirm_state.message().to_string(),
            important: confirm_state.is_important(),
        };
    }

    hint_bar
}

#[cfg(test)]
mod tests {
    use super::*;

    // KeybindingInfo tests
    #[test]
    fn test_keybinding_info_new() {
        let binding = KeybindingInfo::new("ctrl + n", "New");
        assert_eq!(binding.key, "ctrl + n");
        assert_eq!(binding.description, "New");
        assert!(binding.enabled);
    }

    #[test]
    fn test_keybinding_info_disabled() {
        let binding = KeybindingInfo::new("d", "Delete").disabled();
        assert!(!binding.enabled);
    }

    #[test]
    fn test_keybinding_info_display_width() {
        // "q Quit" = 1 + 1 + 4 = 6
        let binding = KeybindingInfo::new("q", "Quit");
        assert_eq!(binding.display_width(), 6);

        // "ctrl + n New" = 8 + 1 + 3 = 12
        let binding = KeybindingInfo::new("ctrl + n", "New");
        assert_eq!(binding.display_width(), 12);
    }

    // HintBarMode tests
    #[test]
    fn test_hint_bar_mode_default() {
        assert_eq!(HintBarMode::default(), HintBarMode::Normal);
    }

    #[test]
    fn test_hint_bar_mode_confirm() {
        let mode = HintBarMode::Confirm {
            message: "Delete?".to_string(),
            important: true,
        };
        match mode {
            HintBarMode::Confirm { message, important } => {
                assert_eq!(message, "Delete?");
                assert!(important);
            }
            _ => panic!("Expected Confirm mode"),
        }
    }

    // HintBar basic tests
    #[test]
    fn test_hint_bar_default() {
        let bar = HintBar::default();
        assert!(bar.bindings.is_empty());
        assert_eq!(bar.mode, HintBarMode::Normal);
        assert!(bar.quit_path.is_empty());
    }

    #[test]
    fn test_hint_bar_new() {
        let bindings = vec![
            KeybindingInfo::new("q", "Quit"),
            KeybindingInfo::new("n", "New"),
        ];
        let bar = HintBar::new(bindings, "q Quit");
        assert_eq!(bar.bindings.len(), 2);
        assert_eq!(bar.quit_path, "q Quit");
    }

    #[test]
    fn test_hint_bar_with_mode() {
        let bar = HintBar::default().with_mode(HintBarMode::Confirm {
            message: "Sure?".to_string(),
            important: false,
        });
        match bar.mode {
            HintBarMode::Confirm { message, important } => {
                assert_eq!(message, "Sure?");
                assert!(!important);
            }
            _ => panic!("Expected Confirm mode"),
        }
    }

    #[test]
    fn test_hint_bar_show_confirm() {
        let mut bar = HintBar::default();
        bar.show_confirm(
            "Delete this?",
            vec![
                KeybindingInfo::new("y", "Yes"),
                KeybindingInfo::new("n", "No"),
            ],
            true,
        );
        match &bar.mode {
            HintBarMode::Confirm { message, important } => {
                assert_eq!(message, "Delete this?");
                assert!(*important);
            }
            _ => panic!("Expected Confirm mode"),
        }
        assert_eq!(bar.bindings.len(), 2);
    }

    #[test]
    fn test_hint_bar_show_message() {
        let mut bar = HintBar::default();
        bar.show_message("Session created!");
        match &bar.mode {
            HintBarMode::Message { text } => {
                assert_eq!(text, "Session created!");
            }
            _ => panic!("Expected Message mode"),
        }
    }

    #[test]
    fn test_hint_bar_set_bindings() {
        let mut bar = HintBar {
            mode: HintBarMode::Confirm {
                message: "test".to_string(),
                important: false,
            },
            ..Default::default()
        };

        bar.set_bindings(vec![KeybindingInfo::new("x", "Exit")]);
        assert_eq!(bar.mode, HintBarMode::Normal);
        assert_eq!(bar.bindings.len(), 1);
    }

    #[test]
    fn test_hint_bar_set_quit_path() {
        let mut bar = HintBar::default();
        bar.set_quit_path("esc → q Quit");
        assert_eq!(bar.quit_path, "esc → q Quit");
    }

    #[test]
    fn test_column_wraps_and_preserves_colors() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("a", "Agent"),
                KeybindingInfo::new("c", "Terminal"),
            ],
            "q Quit",
        );
        assert_eq!(bar.calculate_height(26), 3);
        let area = Rect::new(0, 0, 26, 3);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "a");
        assert_eq!(buf[(0, 1)].symbol(), "c");
        assert_eq!(buf[(0, 2)].symbol(), "q");
        assert_eq!(buf[(0, 0)].fg, colors::PURPLE);
        assert_eq!(buf[(2, 0)].fg, colors::WHITE);
    }

    #[test]
    fn test_column_narrow_unicode_and_confirmation() {
        let bar = HintBar::default().with_mode(HintBarMode::Confirm {
            message: "Delete 界 workspace?".into(),
            important: true,
        });
        assert!(bar.calculate_height(8) > 1);
        for width in 0..28 {
            let area = Rect::new(0, 0, width, 4);
            let mut buf = Buffer::empty(area);
            bar.clone().render(area, &mut buf);
            if width > 0 {
                assert_eq!(buf[(0, 0)].bg, ratatui::style::Color::Reset);
            }
        }
    }

    #[test]
    fn test_exit_path_is_not_duplicated_and_survives_clipping() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("a", "Agent"),
                KeybindingInfo::new("d", "Detach"),
            ],
            "d Detach",
        );
        assert_eq!(bar.calculate_height(26), 2);
        let area = Rect::new(0, 0, 26, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "d");
    }

    #[test]
    fn test_message_replaces_bindings_but_keeps_quit() {
        let mut bar = HintBar::new(vec![KeybindingInfo::new("a", "Agent")], "q Quit");
        bar.show_message("Saved");
        assert_eq!(bar.calculate_height(26), 2);
        let area = Rect::new(0, 0, 26, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "S");
        assert_eq!(buf[(0, 1)].symbol(), "q");
    }

    // Tests for context-aware binding functions
    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_sidebar_focused_welcome() {
        let state = AppState::default();
        let bindings = get_bindings_for_state(&state);

        assert!(
            bindings.iter().any(|b| b.key == "n"),
            "Should have 'n' binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "q"),
            "Should have 'q' binding"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_sidebar_focused_with_sessions() {
        use crate::state::Session;

        let mut state = AppState::default();
        state.sessions.push(Session::new("test"));

        let bindings = get_bindings_for_state(&state);

        assert!(
            bindings.iter().any(|b| b.key == "enter/tab"),
            "Should have 'enter/tab' (select) binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "b/ctrl + b"),
            "Should have 'b/ctrl + b' (jump back) binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "↑/↓/j/k"),
            "Should have vim navigation binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "r"),
            "Should have 'r' (rename) binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "d"),
            "Should have 'd' (delete) binding"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_terminal_focused() {
        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        assert!(
            bindings.iter().any(|b| b.key == "ctrl + n"),
            "Should have 'ctrl + n' binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "ctrl + b"),
            "Should have 'ctrl + b' binding"
        );
    }

    #[test]
    fn test_get_bindings_create_mode() {
        let state = AppState {
            mode: AppMode::CreateMode {
                previous_focus: Focus::Sidebar,
            },
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        assert!(
            bindings.iter().any(|b| b.key == "t"),
            "Should have 't' (terminal) binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "a"),
            "Should have 'a' (agent) binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "esc"),
            "Should have 'esc' binding"
        );
    }

    #[test]
    fn test_get_bindings_drafting_mode() {
        use crate::state::{DraftingState, SessionType};

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        assert!(
            bindings.iter().any(|b| b.key == "enter"),
            "Should have 'enter' binding"
        );
        assert!(
            bindings.iter().any(|b| b.key == "esc"),
            "Should have 'esc' binding"
        );
    }

    #[test]
    fn test_get_bindings_confirming_quit_mode() {
        use crate::state::{ConfirmAction, ConfirmState};

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        // Quit confirmation should show "y/q" as the yes key
        assert!(
            bindings.iter().any(|b| b.key == "y/q"),
            "Should have 'y/q' binding for quit"
        );
        assert!(
            bindings.iter().any(|b| b.key == "n"),
            "Should have 'n' binding"
        );
    }

    #[test]
    fn test_get_bindings_confirming_delete_mode() {
        use crate::state::{ConfirmAction, ConfirmState, Session};

        let state = AppState {
            sessions: vec![Session::new("test")],
            mode: AppMode::Confirming(ConfirmState::new(
                ConfirmAction::DeleteSession(0),
                Focus::Sidebar,
            )),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        // Delete confirmation should show just "y" as the yes key (not "y/q")
        assert!(
            bindings.iter().any(|b| b.key == "y"),
            "Should have 'y' binding for delete"
        );
        assert!(
            !bindings.iter().any(|b| b.key == "y/q"),
            "Should NOT have 'y/q' binding for delete"
        );
        assert!(
            bindings.iter().any(|b| b.key == "n"),
            "Should have 'n' binding"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_quit_path_sidebar_focused() {
        let state = AppState::default();
        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(quit_path, "q Quit");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_quit_path_terminal_focused() {
        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(quit_path, "ctrl + b → q Quit");
    }

    #[test]
    fn test_get_quit_path_drafting_mode() {
        use crate::state::{DraftingState, SessionType};

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(quit_path, "esc → q Quit");
    }

    #[test]
    fn test_get_quit_path_confirming_mode() {
        use crate::state::{ConfirmAction, ConfirmState};

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(quit_path, "n → q Quit");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_hint_bar_for_state_normal() {
        let state = AppState::default();
        let hint_bar = hint_bar_for_state(&state);

        assert_eq!(hint_bar.mode, HintBarMode::Normal);
        assert_eq!(hint_bar.quit_path, "q Quit");
        assert!(!hint_bar.bindings.is_empty());
    }

    #[test]
    fn test_hint_bar_for_state_confirming_important() {
        use crate::state::{ConfirmAction, ConfirmState, Session};

        let state = AppState {
            sessions: vec![Session::new("test")],
            mode: AppMode::Confirming(ConfirmState::new(
                ConfirmAction::DeleteSession(0),
                Focus::Sidebar,
            )),
            ..Default::default()
        };

        let hint_bar = hint_bar_for_state(&state);

        match &hint_bar.mode {
            HintBarMode::Confirm { message, important } => {
                assert!(*important, "Delete should be important");
                assert!(message.contains("Delete"), "Message should mention delete");
            }
            _ => panic!("Should be in Confirm mode"),
        }
    }

    #[test]
    fn test_hint_bar_for_state_confirming_not_important() {
        use crate::state::{ConfirmAction, ConfirmState};

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let hint_bar = hint_bar_for_state(&state);

        match &hint_bar.mode {
            HintBarMode::Confirm { important, .. } => {
                assert!(!important, "Quit should not be important");
            }
            _ => panic!("Should be in Confirm mode"),
        }
    }

    // === Mouse Mode Tests ===

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_shows_text_select_when_mouse_mode_off() {
        let state = AppState {
            focus: Focus::Terminal,
            mouse_mode: false,
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);
        // Should show "Text select" when mouse mode is off
        let mouse_binding = bindings.iter().find(|b| b.key == "ctrl + s");
        assert!(mouse_binding.is_some(), "Should have ctrl + s binding");
        assert_eq!(mouse_binding.unwrap().description, "Text select");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_shows_mouse_scroll_when_mouse_mode_on() {
        let state = AppState {
            focus: Focus::Terminal,
            mouse_mode: true,
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);
        // Should show "Mouse scroll" when mouse mode is on
        let mouse_binding = bindings.iter().find(|b| b.key == "ctrl + s");
        assert!(mouse_binding.is_some(), "Should have ctrl + s binding");
        assert_eq!(mouse_binding.unwrap().description, "Mouse scroll");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_sidebar_shows_mouse_mode() {
        let state = AppState {
            focus: Focus::Sidebar,
            mouse_mode: false,
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);
        // Sidebar should also show mouse mode binding
        let mouse_binding = bindings.iter().find(|b| b.key == "ctrl + s");
        assert!(
            mouse_binding.is_some(),
            "Sidebar should have ctrl + s binding"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_sidebar_with_sessions_shows_mouse_mode() {
        use crate::state::Session;

        let mut state = AppState::default();
        state.sessions.push(Session::new("test"));

        let bindings = get_bindings_for_state(&state);
        let mouse_binding = bindings.iter().find(|b| b.key == "ctrl + s");
        assert!(
            mouse_binding.is_some(),
            "Sidebar with sessions should have ctrl + s binding"
        );
    }

    // === Workspace Overlay Hint Bar Tests ===

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_bindings_workspace_overlay_includes_q_quit() {
        use crate::state::WorkspaceOverlayState;
        let state = AppState {
            mode: AppMode::WorkspaceOverlay(WorkspaceOverlayState::new(
                vec!["Default".to_string()],
                "Default".to_string(),
            )),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);
        assert!(
            bindings
                .iter()
                .any(|b| b.key == "q" && b.description == "Quit"),
            "Workspace overlay bindings should include 'q' for Quit"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_get_quit_path_workspace_overlay_shows_q_quit() {
        use crate::state::WorkspaceOverlayState;
        let state = AppState {
            mode: AppMode::WorkspaceOverlay(WorkspaceOverlayState::new(
                vec!["Default".to_string()],
                "Default".to_string(),
            )),
            ..Default::default()
        };

        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(
            quit_path, "q Quit",
            "Workspace overlay quit path should be 'q Quit'"
        );
    }

    #[test]
    fn test_get_bindings_workspace_overlay_move_mode_includes_q_quit() {
        use crate::state::WorkspaceOverlayState;
        let state = AppState {
            mode: AppMode::WorkspaceOverlay(WorkspaceOverlayState::new_move_mode(
                vec!["Default".to_string()],
                "Default".to_string(),
                "mysession".to_string(),
            )),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);
        assert!(
            bindings
                .iter()
                .any(|b| b.key == "q" && b.description == "Quit"),
            "Workspace overlay move mode bindings should include 'q' for Quit"
        );
        assert!(
            bindings
                .iter()
                .any(|b| b.key == "enter" && b.description == "Move here"),
            "Workspace overlay move mode bindings should include 'enter' for Move here"
        );
        assert!(
            bindings
                .iter()
                .any(|b| b.key == "esc" && b.description == "Cancel"),
            "Workspace overlay move mode bindings should include 'esc' for Cancel"
        );
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_terminal_hint_bar_shows_zoom_binding() {
        let state = AppState {
            focus: Focus::Terminal,
            zoomed: false,
            ..Default::default()
        };
        let bindings = get_bindings_for_state(&state);
        let zoom_binding = bindings.iter().find(|b| b.key == "ctrl + z");
        assert!(
            zoom_binding.is_some(),
            "Terminal bindings should include ctrl + z"
        );
        assert_eq!(zoom_binding.unwrap().description, "Zoom");
    }

    #[test]
    #[ignore = "legacy keybinding expectation replaced by tmux-style binding tests"]
    fn test_terminal_hint_bar_shows_unzoom_when_zoomed() {
        let state = AppState {
            focus: Focus::Terminal,
            zoomed: true,
            ..Default::default()
        };
        let bindings = get_bindings_for_state(&state);
        let zoom_binding = bindings.iter().find(|b| b.key == "ctrl + z");
        assert!(
            zoom_binding.is_some(),
            "Terminal bindings should include ctrl + z when zoomed"
        );
        assert_eq!(zoom_binding.unwrap().description, "Unzoom");
    }
}
