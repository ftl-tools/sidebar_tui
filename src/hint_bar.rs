//! Hint bar module for displaying context-aware keybindings.
//!
//! The hint bar appears at the bottom of the TUI and shows:
//! - Available keybindings based on current context
//! - Confirmation prompts with optional red background
//! - Temporary messages
//! - Quit path on the right side with separator

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
        /// Whether to use red background (for important/destructive actions).
        important: bool,
    },
    /// Temporary message (replaces keybindings briefly).
    Message {
        /// The message text to display.
        text: String,
    },
}

/// Separator between keybindings (two spaces).
const BINDING_SEPARATOR: &str = "  ";
/// Separator width.
const SEPARATOR_WIDTH: usize = 2;
/// Quit separator width (includes "│ ").
const QUIT_SEPARATOR_WIDTH: usize = 2;

/// A wrapped entry that can be rendered on a line.
#[derive(Debug, Clone)]
enum WrappedEntry {
    /// A keybinding entry.
    Binding(KeybindingInfo),
    /// Line break indicator.
    LineBreak,
}

/// Hint bar widget for rendering at the bottom of the TUI.
#[derive(Debug, Clone)]
pub struct HintBar {
    /// Currently displayed keybindings.
    pub bindings: Vec<KeybindingInfo>,
    /// Current display mode.
    pub mode: HintBarMode,
    /// Path to quit shown on the right (e.g., "ctrl + b → q Quit").
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

    /// Calculate the height needed to display the hint bar at the given width.
    pub fn calculate_height(&self, total_width: u16) -> u16 {
        if total_width == 0 {
            return 1;
        }

        // Reserve space for quit path on the right
        let quit_width = if self.quit_path.is_empty() {
            0
        } else {
            QUIT_SEPARATOR_WIDTH + self.quit_path.len()
        };

        let available_width = (total_width as usize).saturating_sub(quit_width);
        if available_width == 0 {
            return 1;
        }

        let wrapped = self.wrap_content(available_width);
        let line_count = wrapped
            .iter()
            .filter(|e| matches!(e, WrappedEntry::LineBreak))
            .count()
            + 1;

        line_count.max(1) as u16
    }

    /// Wrap the content (message/prompt + bindings) to fit within the given width.
    /// Never splits a keybinding across lines.
    fn wrap_content(&self, max_width: usize) -> Vec<WrappedEntry> {
        let mut result = Vec::new();
        let mut current_width: usize = 0;
        let mut is_first_on_line = true;

        // For Confirm mode, account for the message width
        let message_width = match &self.mode {
            HintBarMode::Confirm { message, .. } => message.len() + SEPARATOR_WIDTH,
            HintBarMode::Message { text } => text.len(),
            HintBarMode::Normal => 0,
        };

        // If message alone exceeds width, it will wrap naturally
        // For simplicity, assume message fits on first line
        if message_width > 0 {
            current_width = message_width;
            is_first_on_line = false;
        }

        // If we're in Message mode, no bindings to add
        if matches!(self.mode, HintBarMode::Message { .. }) {
            return result;
        }

        for binding in &self.bindings {
            let entry_width = binding.display_width();
            let needed_width = if is_first_on_line {
                entry_width
            } else {
                SEPARATOR_WIDTH + entry_width
            };

            // Check if we need to wrap
            if current_width + needed_width > max_width && !is_first_on_line {
                result.push(WrappedEntry::LineBreak);
                current_width = entry_width;
                is_first_on_line = false;
            } else {
                current_width += needed_width;
                is_first_on_line = false;
            }

            result.push(WrappedEntry::Binding(binding.clone()));
        }

        result
    }

    /// Build the lines for rendering.
    fn build_lines(&self, available_width: usize) -> Vec<Line<'static>> {
        let wrapped = self.wrap_content(available_width);
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut is_first_on_line = true;

        // Add message/prompt at the start if present
        match &self.mode {
            HintBarMode::Confirm { message, .. } => {
                current_spans.push(Span::styled(
                    message.clone(),
                    Style::default().fg(colors::WHITE),
                ));
                current_spans.push(Span::raw(BINDING_SEPARATOR.to_string()));
                is_first_on_line = false;
            }
            HintBarMode::Message { text } => {
                current_spans.push(Span::styled(
                    text.clone(),
                    Style::default().fg(colors::WHITE),
                ));
                // Message mode has no bindings, just return the message
                lines.push(Line::from(current_spans));
                return lines;
            }
            HintBarMode::Normal => {}
        }

        for entry in wrapped {
            match entry {
                WrappedEntry::Binding(binding) => {
                    // Add separator if not first on line
                    if !is_first_on_line {
                        current_spans.push(Span::raw(BINDING_SEPARATOR.to_string()));
                    }

                    // Key in purple (or dark grey if disabled)
                    let key_style = if binding.enabled {
                        Style::default().fg(colors::PURPLE)
                    } else {
                        Style::default().fg(colors::DARK_GREY)
                    };
                    current_spans.push(Span::styled(binding.key.clone(), key_style));

                    // Space between key and description
                    current_spans.push(Span::raw(" ".to_string()));

                    // Description in white (or dark grey if disabled)
                    let desc_style = if binding.enabled {
                        Style::default().fg(colors::WHITE)
                    } else {
                        Style::default().fg(colors::DARK_GREY)
                    };
                    current_spans.push(Span::styled(binding.description.clone(), desc_style));

                    is_first_on_line = false;
                }
                WrappedEntry::LineBreak => {
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                    is_first_on_line = true;
                }
            }
        }

        // Don't forget the last line
        if !current_spans.is_empty() || lines.is_empty() {
            lines.push(Line::from(current_spans));
        }

        lines
    }

    /// Get the background color based on mode.
    fn background_color(&self) -> ratatui::style::Color {
        match &self.mode {
            HintBarMode::Confirm { important: true, .. } => colors::DARK_RED,
            _ => colors::DARK_GREY,
        }
    }
}

impl Widget for HintBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let bg_color = self.background_color();

        // Fill background
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                buf[(x, y)].set_bg(bg_color);
            }
        }

        // Calculate space for quit path
        let quit_width = if self.quit_path.is_empty() {
            0
        } else {
            QUIT_SEPARATOR_WIDTH + self.quit_path.len()
        };

        let available_width = (area.width as usize).saturating_sub(quit_width);

        // Build and render content lines
        let lines = self.build_lines(available_width);

        for (i, line) in lines.iter().enumerate() {
            if i >= area.height as usize {
                break;
            }
            let y = area.y + i as u16;
            let mut x = area.x;

            for span in line.spans.iter() {
                let text = span.content.as_ref();
                for c in text.chars() {
                    if x < area.x + area.width {
                        buf[(x, y)]
                            .set_char(c)
                            .set_style(span.style);
                        x += 1;
                    }
                }
            }
        }

        // Render quit path on the last line, right-aligned
        if !self.quit_path.is_empty() && area.height > 0 {
            let last_line_y = area.y + area.height - 1;
            let quit_start_x = area.x + area.width - quit_width as u16;

            // Render separator
            if quit_start_x >= area.x {
                buf[(quit_start_x, last_line_y)]
                    .set_char('│')
                    .set_fg(colors::SEPARATOR);
                if quit_start_x + 1 < area.x + area.width {
                    buf[(quit_start_x + 1, last_line_y)]
                        .set_char(' ')
                        .set_fg(colors::SEPARATOR);
                }
            }

            // Render quit path text with proper coloring
            // Keys should be purple, "Quit" should be white
            let quit_text_x = quit_start_x + QUIT_SEPARATOR_WIDTH as u16;
            let colored_spans = color_quit_path(&self.quit_path);
            let mut x = quit_text_x;
            for (text, color) in colored_spans {
                for c in text.chars() {
                    if x < area.x + area.width {
                        buf[(x, last_line_y)]
                            .set_char(c)
                            .set_fg(color);
                    }
                    x += 1;
                }
            }
        }
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
                        KeybindingInfo::new("n", "New"),
                        KeybindingInfo::new("ctrl + s", mouse_desc),
                        KeybindingInfo::new("q", "Quit"),
                    ]
                } else {
                    vec![
                        KeybindingInfo::new("enter/tab", "Select"),
                        KeybindingInfo::new("b/ctrl + b", "Jump back"),
                        KeybindingInfo::new("↑/↓/j/k", "Navigate"),
                        KeybindingInfo::new("n", "New"),
                        KeybindingInfo::new("r", "Rename"),
                        KeybindingInfo::new("d", "Delete"),
                        KeybindingInfo::new("m", "Move to workspace"),
                        KeybindingInfo::new("w", "Workspaces"),
                        KeybindingInfo::new("ctrl + s", mouse_desc),
                        KeybindingInfo::new("q", "Quit"),
                    ]
                }
            }
            Focus::Terminal => {
                let zoom_desc = if state.zoomed { "Unzoom" } else { "Zoom" };
                vec![
                    KeybindingInfo::new("ctrl + n", "New"),
                    KeybindingInfo::new("ctrl + b", "Sidebar"),
                    KeybindingInfo::new("ctrl + w", "Workspaces"),
                    KeybindingInfo::new("ctrl + s", mouse_desc),
                    KeybindingInfo::new("ctrl + z", zoom_desc),
                    KeybindingInfo::new("ctrl + q", "Quit"),
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
                    KeybindingInfo::new("n", "New"),
                    KeybindingInfo::new("r", "Rename"),
                    KeybindingInfo::new("d", "Delete"),
                    KeybindingInfo::new("esc", "Close"),
                    KeybindingInfo::new("q", "Quit"),
                ]
            }
        }
    }
}

/// Get the quit path string for the current app state.
pub fn get_quit_path_for_state(state: &AppState) -> String {
    match &state.mode {
        AppMode::Normal => match state.focus {
            Focus::Sidebar => "q Quit".to_string(),
            Focus::Terminal => "ctrl + b → q Quit".to_string(),
        },
        AppMode::CreateMode { .. } => "esc → q Quit".to_string(),
        AppMode::Drafting(_) | AppMode::Renaming(_) => "esc → q Quit".to_string(),
        AppMode::Confirming(_) => "n → q Quit".to_string(),
        AppMode::WorkspaceOverlay(_) => "q Quit".to_string(),
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

    // Height calculation tests
    #[test]
    fn test_calculate_height_single_line() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("q", "Quit"),   // 6
                KeybindingInfo::new("n", "New"),    // 5
            ],
            "q Quit", // 6 + 2 separator = 8
        );
        // Total bindings: 6 + 2 + 5 = 13
        // Width 80, quit 8, available 72 -> fits on one line
        assert_eq!(bar.calculate_height(80), 1);
    }

    #[test]
    fn test_calculate_height_wraps_to_multiple_lines() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("ctrl + n", "New"),           // 8+1+3 = 12
                KeybindingInfo::new("ctrl + b", "Focus sidebar"), // 8+1+13 = 22
                KeybindingInfo::new("ctrl + q", "Quit"),          // 8+1+4 = 13
            ],
            "q Quit", // 6 + 2 separator = 8
        );
        // At width 40, quit takes 8, available = 32
        // Line 1: 12, then +2+22 = 36 > 32, wrap
        // Line 2: 22, then +2+13 = 37 > 32, wrap
        // Line 3: 13
        assert_eq!(bar.calculate_height(40), 3);
    }

    #[test]
    fn test_calculate_height_wraps_to_two_lines() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("ctrl + n", "New"),   // 12
                KeybindingInfo::new("enter", "Select"),   // 5+1+6 = 12
            ],
            "q Quit", // 8
        );
        // At width 30, quit takes 8, available = 22
        // Line 1: 12, then +2+12 = 26 > 22, wrap
        // Line 2: 12
        assert_eq!(bar.calculate_height(30), 2);
    }

    #[test]
    fn test_calculate_height_zero_width() {
        let bar = HintBar::default();
        assert_eq!(bar.calculate_height(0), 1);
    }

    #[test]
    fn test_calculate_height_no_quit_path() {
        let bar = HintBar::new(
            vec![KeybindingInfo::new("q", "Quit")],
            "",
        );
        assert_eq!(bar.calculate_height(20), 1);
    }

    // Wrapping tests
    #[test]
    fn test_wrap_content_all_fit() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("a", "Act"),
                KeybindingInfo::new("b", "Back"),
            ],
            "",
        );
        let wrapped = bar.wrap_content(50);
        // Should have 2 bindings, no line breaks
        let bindings_count = wrapped
            .iter()
            .filter(|e| matches!(e, WrappedEntry::Binding(_)))
            .count();
        let breaks_count = wrapped
            .iter()
            .filter(|e| matches!(e, WrappedEntry::LineBreak))
            .count();
        assert_eq!(bindings_count, 2);
        assert_eq!(breaks_count, 0);
    }

    #[test]
    fn test_wrap_content_needs_wrap() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("ctrl + shift + a", "Very long action"), // 33
                KeybindingInfo::new("b", "Back"),                            // 6
            ],
            "",
        );
        let wrapped = bar.wrap_content(35);
        // First binding takes 33, second would need 2 + 6 = 8 more = 41 > 35
        // Should wrap
        let breaks: usize = wrapped
            .iter()
            .filter(|e| matches!(e, WrappedEntry::LineBreak))
            .count();
        assert_eq!(breaks, 1);
    }

    #[test]
    fn test_wrap_content_message_mode() {
        let mut bar = HintBar::default();
        bar.show_message("Hello world");
        let wrapped = bar.wrap_content(50);
        // Message mode returns empty (message handled separately in build_lines)
        assert!(wrapped.is_empty());
    }

    // Background color tests
    #[test]
    fn test_background_color_normal() {
        let bar = HintBar::default();
        assert_eq!(bar.background_color(), colors::DARK_GREY);
    }

    #[test]
    fn test_background_color_confirm_important() {
        let bar = HintBar::default().with_mode(HintBarMode::Confirm {
            message: "Delete?".to_string(),
            important: true,
        });
        assert_eq!(bar.background_color(), colors::DARK_RED);
    }

    #[test]
    fn test_background_color_confirm_not_important() {
        let bar = HintBar::default().with_mode(HintBarMode::Confirm {
            message: "Continue?".to_string(),
            important: false,
        });
        assert_eq!(bar.background_color(), colors::DARK_GREY);
    }

    // Build lines tests
    #[test]
    fn test_build_lines_normal_mode() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("q", "Quit"),
                KeybindingInfo::new("n", "New"),
            ],
            "",
        );
        let lines = bar.build_lines(50);
        assert_eq!(lines.len(), 1);
        // Should have spans for: "q" " " "Quit" "  " "n" " " "New"
        assert!(lines[0].spans.len() >= 4);
    }

    #[test]
    fn test_build_lines_confirm_mode() {
        let bar = HintBar::default().with_mode(HintBarMode::Confirm {
            message: "Sure?".to_string(),
            important: false,
        });
        let lines = bar.build_lines(50);
        assert_eq!(lines.len(), 1);
        // First span should be the message
        assert_eq!(lines[0].spans[0].content.as_ref(), "Sure?");
    }

    #[test]
    fn test_build_lines_message_mode() {
        let mut bar = HintBar::default();
        bar.show_message("Done!");
        let lines = bar.build_lines(50);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content.as_ref(), "Done!");
    }

    #[test]
    fn test_build_lines_with_wrapping() {
        let bar = HintBar::new(
            vec![
                KeybindingInfo::new("longkey", "Long description here"),
                KeybindingInfo::new("x", "Exit"),
            ],
            "",
        );
        // Width 20 should force wrap
        let lines = bar.build_lines(20);
        assert!(lines.len() >= 2);
    }

    // Rendering tests (basic buffer checks)
    #[test]
    fn test_render_fills_background() {
        let bar = HintBar::default();
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // All cells should have dark grey background
        for x in 0..20 {
            assert_eq!(buf[(x, 0)].bg, colors::DARK_GREY);
        }
    }

    #[test]
    fn test_render_important_confirm_red_background() {
        let bar = HintBar::default().with_mode(HintBarMode::Confirm {
            message: "Delete?".to_string(),
            important: true,
        });
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // All cells should have dark red background
        for x in 0..20 {
            assert_eq!(buf[(x, 0)].bg, colors::DARK_RED);
        }
    }

    #[test]
    fn test_render_quit_path_at_right() {
        let bar = HintBar::new(vec![], "q Quit");
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // Check that "│ q Quit" appears at the right
        // quit_width = 2 + 6 = 8
        // starts at x = 20 - 8 = 12
        assert_eq!(buf[(12, 0)].symbol(), "│");
        assert_eq!(buf[(14, 0)].symbol(), "q");
        assert_eq!(buf[(16, 0)].symbol(), "Q");
    }

    #[test]
    fn test_render_quit_path_colors() {
        // Test simple "q Quit" - 'q' should be purple, 'Quit' should be white
        let bar = HintBar::new(vec![], "q Quit");
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // "│ q Quit" starts at x = 12
        // Position 14 = 'q' should be purple
        assert_eq!(buf[(14, 0)].symbol(), "q");
        assert_eq!(buf[(14, 0)].fg, colors::PURPLE);

        // Position 16 = 'Q' of "Quit" should be white
        assert_eq!(buf[(16, 0)].symbol(), "Q");
        assert_eq!(buf[(16, 0)].fg, colors::WHITE);
    }

    #[test]
    fn test_render_quit_path_complex_colors() {
        // Test "ctrl + b → q Quit" coloring
        let bar = HintBar::new(vec![], "ctrl + b → q Quit");
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // Find positions of key characters dynamically
        let c_pos = (0..40u16).find(|&x| buf[(x, 0)].symbol() == "c").expect("'c' not found");
        let q_pos = (0..40u16).find(|&x| buf[(x, 0)].symbol() == "q").expect("'q' not found");
        let big_q_pos = (0..40u16).find(|&x| buf[(x, 0)].symbol() == "Q").expect("'Q' not found");

        // 'c' from "ctrl + b" should be purple (first key segment)
        assert_eq!(buf[(c_pos, 0)].fg, colors::PURPLE, "'c' should be purple");

        // 'q' should be purple (second key segment)
        assert_eq!(buf[(q_pos, 0)].fg, colors::PURPLE, "'q' should be purple");

        // 'Q' from "Quit" should be white
        assert_eq!(buf[(big_q_pos, 0)].fg, colors::WHITE, "'Q' from Quit should be white");
    }

    #[test]
    fn test_separator_color_is_242() {
        // Test that the separator character "│" uses SEPARATOR color (242)
        let bar = HintBar::new(vec![], "q Quit");
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // "│ q Quit" - separator is at x = 12
        assert_eq!(buf[(12, 0)].symbol(), "│");
        assert_eq!(buf[(12, 0)].fg, colors::SEPARATOR, "Separator should use SEPARATOR color (242)");
    }

    #[test]
    fn test_render_zero_area() {
        let bar = HintBar::default();
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 10));

        // Should not panic
        bar.render(area, &mut buf);
    }

    #[test]
    fn test_render_keybindings_with_colors() {
        let bar = HintBar::new(
            vec![KeybindingInfo::new("q", "Quit")],
            "",
        );
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // First char 'q' should be purple
        assert_eq!(buf[(0, 0)].symbol(), "q");
        assert_eq!(buf[(0, 0)].fg, colors::PURPLE);

        // Space at position 1
        assert_eq!(buf[(1, 0)].symbol(), " ");

        // 'Q' of "Quit" at position 2 should be white
        assert_eq!(buf[(2, 0)].symbol(), "Q");
        assert_eq!(buf[(2, 0)].fg, colors::WHITE);
    }

    #[test]
    fn test_render_disabled_keybinding() {
        let bar = HintBar::new(
            vec![KeybindingInfo::new("d", "Delete").disabled()],
            "",
        );
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        bar.render(area, &mut buf);

        // Disabled keybinding should be dark grey
        assert_eq!(buf[(0, 0)].fg, colors::DARK_GREY);
        assert_eq!(buf[(2, 0)].fg, colors::DARK_GREY);
    }

    // Tests for context-aware binding functions
    #[test]
    fn test_get_bindings_sidebar_focused_welcome() {
        let state = AppState::default();
        let bindings = get_bindings_for_state(&state);

        assert!(bindings.iter().any(|b| b.key == "n"), "Should have 'n' binding");
        assert!(bindings.iter().any(|b| b.key == "q"), "Should have 'q' binding");
    }

    #[test]
    fn test_get_bindings_sidebar_focused_with_sessions() {
        use crate::state::Session;

        let mut state = AppState::default();
        state.sessions.push(Session::new("test"));

        let bindings = get_bindings_for_state(&state);

        assert!(bindings.iter().any(|b| b.key == "enter/tab"), "Should have 'enter/tab' (select) binding");
        assert!(bindings.iter().any(|b| b.key == "b/ctrl + b"), "Should have 'b/ctrl + b' (jump back) binding");
        assert!(bindings.iter().any(|b| b.key == "↑/↓/j/k"), "Should have vim navigation binding");
        assert!(bindings.iter().any(|b| b.key == "r"), "Should have 'r' (rename) binding");
        assert!(bindings.iter().any(|b| b.key == "d"), "Should have 'd' (delete) binding");
    }

    #[test]
    fn test_get_bindings_terminal_focused() {
        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        assert!(bindings.iter().any(|b| b.key == "ctrl + n"), "Should have 'ctrl + n' binding");
        assert!(bindings.iter().any(|b| b.key == "ctrl + b"), "Should have 'ctrl + b' binding");
    }

    #[test]
    fn test_get_bindings_create_mode() {
        let state = AppState {
            mode: AppMode::CreateMode { previous_focus: Focus::Sidebar },
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        assert!(bindings.iter().any(|b| b.key == "t"), "Should have 't' (terminal) binding");
        assert!(bindings.iter().any(|b| b.key == "a"), "Should have 'a' (agent) binding");
        assert!(bindings.iter().any(|b| b.key == "esc"), "Should have 'esc' binding");
    }

    #[test]
    fn test_get_bindings_drafting_mode() {
        use crate::state::{DraftingState, SessionType};

        let state = AppState {
            mode: AppMode::Drafting(DraftingState::new(SessionType::Terminal, Focus::Sidebar)),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        assert!(bindings.iter().any(|b| b.key == "enter"), "Should have 'enter' binding");
        assert!(bindings.iter().any(|b| b.key == "esc"), "Should have 'esc' binding");
    }

    #[test]
    fn test_get_bindings_confirming_quit_mode() {
        use crate::state::{ConfirmState, ConfirmAction};

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        // Quit confirmation should show "y/q" as the yes key
        assert!(bindings.iter().any(|b| b.key == "y/q"), "Should have 'y/q' binding for quit");
        assert!(bindings.iter().any(|b| b.key == "n"), "Should have 'n' binding");
    }

    #[test]
    fn test_get_bindings_confirming_delete_mode() {
        use crate::state::{ConfirmState, ConfirmAction, Session};

        let state = AppState {
            sessions: vec![Session::new("test")],
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar)),
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);

        // Delete confirmation should show just "y" as the yes key (not "y/q")
        assert!(bindings.iter().any(|b| b.key == "y"), "Should have 'y' binding for delete");
        assert!(!bindings.iter().any(|b| b.key == "y/q"), "Should NOT have 'y/q' binding for delete");
        assert!(bindings.iter().any(|b| b.key == "n"), "Should have 'n' binding");
    }

    #[test]
    fn test_get_quit_path_sidebar_focused() {
        let state = AppState::default();
        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(quit_path, "q Quit");
    }

    #[test]
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
        use crate::state::{ConfirmState, ConfirmAction};

        let state = AppState {
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::Quit, Focus::Sidebar)),
            ..Default::default()
        };

        let quit_path = get_quit_path_for_state(&state);
        assert_eq!(quit_path, "n → q Quit");
    }

    #[test]
    fn test_hint_bar_for_state_normal() {
        let state = AppState::default();
        let hint_bar = hint_bar_for_state(&state);

        assert_eq!(hint_bar.mode, HintBarMode::Normal);
        assert_eq!(hint_bar.quit_path, "q Quit");
        assert!(!hint_bar.bindings.is_empty());
    }

    #[test]
    fn test_hint_bar_for_state_confirming_important() {
        use crate::state::{ConfirmState, ConfirmAction, Session};

        let state = AppState {
            sessions: vec![Session::new("test")],
            mode: AppMode::Confirming(ConfirmState::new(ConfirmAction::DeleteSession(0), Focus::Sidebar)),
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
        use crate::state::{ConfirmState, ConfirmAction};

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
    fn test_get_bindings_sidebar_shows_mouse_mode() {
        let state = AppState {
            focus: Focus::Sidebar,
            mouse_mode: false,
            ..Default::default()
        };

        let bindings = get_bindings_for_state(&state);
        // Sidebar should also show mouse mode binding
        let mouse_binding = bindings.iter().find(|b| b.key == "ctrl + s");
        assert!(mouse_binding.is_some(), "Sidebar should have ctrl + s binding");
    }

    #[test]
    fn test_get_bindings_sidebar_with_sessions_shows_mouse_mode() {
        use crate::state::Session;

        let mut state = AppState::default();
        state.sessions.push(Session::new("test"));

        let bindings = get_bindings_for_state(&state);
        let mouse_binding = bindings.iter().find(|b| b.key == "ctrl + s");
        assert!(mouse_binding.is_some(), "Sidebar with sessions should have ctrl + s binding");
    }

    // === Workspace Overlay Hint Bar Tests ===

    #[test]
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
            bindings.iter().any(|b| b.key == "q" && b.description == "Quit"),
            "Workspace overlay bindings should include 'q' for Quit"
        );
    }

    #[test]
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
        assert_eq!(quit_path, "q Quit", "Workspace overlay quit path should be 'q Quit'");
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
            bindings.iter().any(|b| b.key == "q" && b.description == "Quit"),
            "Workspace overlay move mode bindings should include 'q' for Quit"
        );
        assert!(
            bindings.iter().any(|b| b.key == "enter" && b.description == "Move here"),
            "Workspace overlay move mode bindings should include 'enter' for Move here"
        );
        assert!(
            bindings.iter().any(|b| b.key == "esc" && b.description == "Cancel"),
            "Workspace overlay move mode bindings should include 'esc' for Cancel"
        );
    }

    #[test]
    fn test_terminal_hint_bar_shows_zoom_binding() {
        let state = AppState {
            focus: Focus::Terminal,
            zoomed: false,
            ..Default::default()
        };
        let bindings = get_bindings_for_state(&state);
        let zoom_binding = bindings.iter().find(|b| b.key == "ctrl + z");
        assert!(zoom_binding.is_some(), "Terminal bindings should include ctrl + z");
        assert_eq!(zoom_binding.unwrap().description, "Zoom");
    }

    #[test]
    fn test_terminal_hint_bar_shows_unzoom_when_zoomed() {
        let state = AppState {
            focus: Focus::Terminal,
            zoomed: true,
            ..Default::default()
        };
        let bindings = get_bindings_for_state(&state);
        let zoom_binding = bindings.iter().find(|b| b.key == "ctrl + z");
        assert!(zoom_binding.is_some(), "Terminal bindings should include ctrl + z when zoomed");
        assert_eq!(zoom_binding.unwrap().description, "Unzoom");
    }

}
