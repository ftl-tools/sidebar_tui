//! Sidebar pane rendering for the Sidebar TUI.
//!
//! This module handles rendering the sidebar with:
//! - Session list with selection and scrolling
//! - Line wrapping for long session names with continuation indicators
//! - Focus-aware border colors
//! - Truncation indicators when list overflows
//! - Welcome state when no sessions exist

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Widget};

use crate::colors::{DARK_GREY, FOCUSED_BORDER, PURPLE, WHITE};
use crate::state::{AppMode, AppState, Focus};

/// Width of the sidebar pane including borders.
pub const SIDEBAR_WIDTH: u16 = 28;

/// Padding on each side between border and content.
const PADDING: u16 = 1;

/// Content width inside the sidebar (excluding borders and padding).
/// SIDEBAR_WIDTH (28) - 2 (left/right borders) - 2 (left/right padding) = 24 chars for content.
const CONTENT_WIDTH: usize = (SIDEBAR_WIDTH - 2 - PADDING * 2) as usize;

/// Continuation indicator for wrapped lines (not the first line).
const CONTINUATION_MIDDLE: &str = "│";
/// Final continuation indicator for the last line of a wrapped name.
const CONTINUATION_END: &str = "└";

/// A rendered line in the sidebar representing a session name (or part of it).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionLine {
    /// The text content to display.
    text: String,
    /// Session index this line belongs to (usize::MAX for draft).
    session_index: usize,
    /// Whether this is a continuation line (not the first line of the name).
    is_continuation: bool,
    /// Whether this is the last line of a wrapped name.
    is_last_line: bool,
}

/// Wrap a session name into multiple lines if needed.
/// Returns a vector of (text, is_continuation, is_last_line) tuples.
fn wrap_session_name(name: &str, max_width: usize) -> Vec<(String, bool, bool)> {
    if name.is_empty() {
        return vec![(String::new(), false, true)];
    }

    let mut lines = Vec::new();
    let mut remaining = name;
    let first_line_width = max_width;
    // Continuation lines need space for the indicator (│ or └)
    let continuation_width = max_width.saturating_sub(1);

    // First line
    if remaining.len() <= first_line_width {
        lines.push((remaining.to_string(), false, true));
        return lines;
    }

    // Split at first_line_width
    let (first, rest) = remaining.split_at(first_line_width);
    lines.push((first.to_string(), false, false));
    remaining = rest;

    // Continuation lines
    while !remaining.is_empty() {
        if remaining.len() <= continuation_width {
            lines.push((remaining.to_string(), true, true));
            break;
        }
        let (chunk, rest) = remaining.split_at(continuation_width);
        lines.push((chunk.to_string(), true, false));
        remaining = rest;
    }

    lines
}

/// Calculate how many visual rows a session name will take.
fn session_row_count(name: &str, max_width: usize) -> usize {
    wrap_session_name(name, max_width).len()
}

/// Given wrapped lines and a cursor_position (character offset into the full string),
/// return (line_index, col_within_that_line).
fn cursor_line_col(wrapped: &[(String, bool, bool)], cursor_position: usize) -> (usize, usize) {
    let mut chars_before = 0usize;
    for (line_idx, (text, _, _)) in wrapped.iter().enumerate() {
        let chars_on_line = text.len();
        if cursor_position <= chars_before + chars_on_line {
            return (line_idx, cursor_position - chars_before);
        }
        if line_idx == wrapped.len().saturating_sub(1) {
            return (line_idx, chars_on_line);
        }
        chars_before += chars_on_line;
    }
    (0, 0)
}

/// Build the list of sidebar lines for rendering.
/// Returns (lines, show_top_truncation, show_bottom_truncation).
fn build_sidebar_lines(
    state: &AppState,
    visible_rows: usize,
) -> (Vec<SessionLine>, bool, bool) {
    let max_width = CONTENT_WIDTH;
    let mut lines = Vec::new();
    let mut show_top_truncation = false;
    let mut show_bottom_truncation = false;

    // Handle drafting mode - add empty draft row at top
    let draft_name = if let AppMode::Drafting(draft) = &state.mode {
        Some(draft.name.clone())
    } else {
        None
    };

    // Rename info: (session_index, new_name) if we're currently renaming a session.
    // We use the rename text for row calculations so wrapping reflects what's being typed.
    let renaming_info: Option<(usize, String)> = if let AppMode::Renaming(rename) = &state.mode {
        Some((rename.session_index, rename.new_name.clone()))
    } else {
        None
    };

    // Helper: get the display name for session at index (substitutes rename text if applicable).
    let effective_name = |idx: usize, session_name: &str| -> String {
        if let Some((ri, ref rn)) = renaming_info {
            if ri == idx {
                return rn.clone();
            }
        }
        session_name.to_string()
    };

    // Calculate total rows needed for all sessions (plus draft if any)
    let mut total_rows = 0;
    if draft_name.is_some() {
        total_rows += session_row_count("", max_width);
    }
    for (idx, session) in state.sessions.iter().enumerate() {
        total_rows += session_row_count(&effective_name(idx, &session.name), max_width);
    }

    // If everything fits, render all
    if total_rows <= visible_rows {
        // Render draft if present
        if let Some(ref name) = draft_name {
            for (text, is_continuation, is_last_line) in wrap_session_name(name, max_width) {
                lines.push(SessionLine {
                    text,
                    session_index: usize::MAX, // Special marker for draft
                    is_continuation,
                    is_last_line,
                });
            }
        }
        // Render all sessions (using effective name for renamed sessions)
        for (idx, session) in state.sessions.iter().enumerate() {
            let name = effective_name(idx, &session.name);
            for (text, is_continuation, is_last_line) in wrap_session_name(&name, max_width) {
                lines.push(SessionLine {
                    text,
                    session_index: idx,
                    is_continuation,
                    is_last_line,
                });
            }
        }
        return (lines, false, false);
    }

    // Need scrolling - use scroll_offset to determine what to show
    // We need to account for truncation indicators taking up space
    let available_rows = visible_rows.saturating_sub(2); // Reserve space for potential truncation indicators

    // Calculate which sessions are visible based on scroll_offset
    let scroll_offset = state.scroll_offset;
    let mut rows_before_scroll = 0;
    let mut first_visible_session = 0;

    // Account for draft in scrolling
    if draft_name.is_some() {
        let draft_rows = session_row_count("", max_width);
        if scroll_offset > 0 {
            rows_before_scroll = draft_rows;
            show_top_truncation = true;
        }
    }

    // Find first visible session (using effective names for row counts)
    for (idx, session) in state.sessions.iter().enumerate() {
        let name = effective_name(idx, &session.name);
        let session_rows = session_row_count(&name, max_width);
        if rows_before_scroll + session_rows > scroll_offset {
            first_visible_session = idx;
            break;
        }
        rows_before_scroll += session_rows;
        if idx + 1 < state.sessions.len() || draft_name.is_some() {
            show_top_truncation = true;
        }
    }

    // Determine rows available (accounting for top truncation indicator)
    let rows_for_content = if show_top_truncation {
        available_rows.saturating_sub(1)
    } else {
        available_rows
    };

    // Build visible lines
    let mut rows_used = 0;

    // If draft is visible (scroll_offset == 0 and draft exists)
    if draft_name.is_some() && scroll_offset == 0 {
        let wrapped = wrap_session_name(&draft_name.clone().unwrap(), max_width);
        for (text, is_continuation, is_last_line) in wrapped {
            if rows_used >= rows_for_content {
                show_bottom_truncation = true;
                break;
            }
            lines.push(SessionLine {
                text,
                session_index: usize::MAX,
                is_continuation,
                is_last_line,
            });
            rows_used += 1;
        }
    }

    // Add visible sessions (using effective names for renamed sessions)
    for idx in first_visible_session..state.sessions.len() {
        if rows_used >= rows_for_content {
            show_bottom_truncation = true;
            break;
        }
        let session = &state.sessions[idx];
        let name = effective_name(idx, &session.name);
        let wrapped = wrap_session_name(&name, max_width);
        for (text, is_continuation, is_last_line) in wrapped {
            if rows_used >= rows_for_content {
                show_bottom_truncation = true;
                break;
            }
            lines.push(SessionLine {
                text,
                session_index: idx,
                is_continuation,
                is_last_line,
            });
            rows_used += 1;
        }
    }

    (lines, show_top_truncation, show_bottom_truncation)
}

/// Widget for rendering the sidebar.
pub struct Sidebar<'a> {
    state: &'a AppState,
}

impl<'a> Sidebar<'a> {
    /// Create a new Sidebar widget.
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    /// Render the sidebar title (current workspace name).
    fn render_title(&self, buf: &mut Buffer, area: Rect) {
        // Title shows current workspace name in purple, left-aligned with padding.
        // Truncate with "..." if the name is too long to fit.
        let max_width = area.width.saturating_sub(PADDING * 2) as usize;
        let title = if self.state.workspace_name.len() > max_width {
            let truncated = &self.state.workspace_name[..max_width.saturating_sub(3)];
            format!("{}...", truncated)
        } else {
            self.state.workspace_name.clone()
        };
        let style = Style::default().fg(PURPLE);
        buf.set_string(area.x + PADDING, area.y, &title, style);
    }

    /// Render the welcome state message.
    fn render_welcome(&self, buf: &mut Buffer, area: Rect) {
        // Center the welcome message in the available area (accounting for padding)
        // The message should be colored grey (238) with purple keybinding
        let key = if self.state.focus == Focus::Sidebar {
            "n"
        } else {
            "ctrl+n"
        };

        // Calculate vertical centering
        let total_lines = 6; // Welcome, Sidebar TUI!, blank, Press, key, to create...
        let start_y = area.y + (area.height.saturating_sub(total_lines as u16)) / 2;

        // Render each line centered
        let lines_to_render = [
            vec![Span::styled("Welcome to", Style::default().fg(DARK_GREY))],
            vec![Span::styled("Sidebar TUI!", Style::default().fg(DARK_GREY))],
            vec![], // blank line
            vec![
                Span::styled("Press ", Style::default().fg(DARK_GREY)),
                Span::styled(key, Style::default().fg(PURPLE)),
            ],
            vec![Span::styled("to create your", Style::default().fg(DARK_GREY))],
            vec![Span::styled("first session!", Style::default().fg(DARK_GREY))],
        ];

        // Content area starts after padding on left and ends before padding on right
        let content_x = area.x + PADDING;
        let content_width = area.width.saturating_sub(PADDING * 2);

        for (i, spans) in lines_to_render.iter().enumerate() {
            let y = start_y + i as u16;
            if y >= area.y + area.height {
                break;
            }

            // Calculate total width for centering within content area
            let total_width: usize = spans.iter().map(|s| s.content.len()).sum();
            let x = content_x + (content_width.saturating_sub(total_width as u16)) / 2;

            let mut current_x = x;
            for span in spans {
                buf.set_string(current_x, y, &span.content, span.style);
                current_x += span.content.len() as u16;
            }
        }
    }

    /// Render the session list.
    fn render_session_list(&self, buf: &mut Buffer, area: Rect) {
        let visible_rows = area.height as usize;
        let (lines, show_top, show_bottom) = build_sidebar_lines(self.state, visible_rows);

        let mut y = area.y;
        // Content starts after padding
        let content_x = area.x + PADDING;

        // Top truncation indicator
        if show_top {
            let indicator = "...";
            buf.set_string(content_x, y, indicator, Style::default().fg(DARK_GREY));
            y += 1;
        }

        // Check if we're in drafting mode
        let is_drafting = matches!(&self.state.mode, AppMode::Drafting(_));

        // Render session lines
        for line in &lines {
            if y >= area.y + area.height {
                break;
            }

            let SessionLine {
                text,
                session_index,
                is_continuation,
                is_last_line,
            } = line;

            // Determine if this line is selected
            let is_selected = if *session_index == usize::MAX {
                // This is the draft line - always selected while drafting
                is_drafting
            } else {
                *session_index == self.state.selected_index && !is_drafting
            };

            // Background style for selection
            // Per user request: use grey (238) instead of purple for selection highlight
            let bg_style = if is_selected {
                Style::default().bg(DARK_GREY)
            } else {
                Style::default()
            };

            // Fill the line with background color if selected
            // Per spec: highlight starts at first letter (content_x) and stops right before the right sidebar border
            if is_selected {
                // area.width includes both left and right padding, so subtract PADDING to stop before right padding
                for x in content_x..area.x + area.width - PADDING {
                    buf[(x, y)].set_style(bg_style);
                }
            }

            // Render continuation indicator (with padding)
            let mut x = content_x;
            if *is_continuation {
                let indicator = if *is_last_line {
                    CONTINUATION_END
                } else {
                    CONTINUATION_MIDDLE
                };
                buf.set_string(x, y, indicator, Style::default().fg(DARK_GREY));
                x += 1;
            }

            // Render text: always use the pre-wrapped slice for this line.
            // For draft and renamed sessions, build_sidebar_lines already wraps using the
            // typed text, so 'text' is the correct slice to show here.
            let text_style = if is_selected {
                Style::default().fg(WHITE).bg(DARK_GREY)
            } else {
                Style::default().fg(WHITE)
            };
            buf.set_string(x, y, text, text_style);

            y += 1;
        }

        // Bottom truncation indicator
        if show_bottom && y < area.y + area.height {
            let indicator = "...";
            buf.set_string(content_x, y, indicator, Style::default().fg(DARK_GREY));
        }
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Determine border color based on focus
        let border_color = if self.state.focus == Focus::Sidebar
            || matches!(self.state.mode, AppMode::Drafting(_) | AppMode::Renaming(_))
        {
            FOCUSED_BORDER
        } else {
            DARK_GREY
        };

        // Create the block with border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        // Render the block
        let inner = block.inner(area);
        block.render(area, buf);

        // Render title on the first row inside the border
        if inner.height > 0 {
            self.render_title(buf, Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 1,
            });
        }

        // Content area (below title)
        let content_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };

        if self.state.is_welcome_state() && !matches!(self.state.mode, AppMode::Drafting(_)) {
            self.render_welcome(buf, content_area);
        } else {
            self.render_session_list(buf, content_area);
        }
    }
}

/// Get cursor position for drafting or renaming mode.
/// Returns (x, y) position if cursor should be shown.
pub fn get_sidebar_cursor_position(state: &AppState, area: Rect) -> Option<(u16, u16)> {
    let inner_x = area.x + 1 + PADDING; // Inside border + padding
    let inner_y = area.y + 2; // Below border and title

    match &state.mode {
        AppMode::Drafting(draft) => {
            // Cursor may be on a wrapped line; compute which line and column.
            let wrapped = wrap_session_name(&draft.name, CONTENT_WIDTH);
            let (cursor_line, cursor_col) = cursor_line_col(&wrapped, draft.cursor_position);
            let indicator_offset = if cursor_line > 0 { 1u16 } else { 0u16 };
            let cursor_x = inner_x + indicator_offset + cursor_col as u16;
            let cursor_y = inner_y + cursor_line as u16;
            Some((cursor_x, cursor_y))
        }
        AppMode::Renaming(rename) => {
            // Rows before the renamed session (using the current rename text for that session's row count).
            let rows_before: usize = state.sessions.iter()
                .take(rename.session_index)
                .map(|s| session_row_count(&s.name, CONTENT_WIDTH))
                .sum();
            // Within the renamed session, cursor may be on a wrapped line.
            let wrapped = wrap_session_name(&rename.new_name, CONTENT_WIDTH);
            let (cursor_line, cursor_col) = cursor_line_col(&wrapped, rename.cursor_position);
            let indicator_offset = if cursor_line > 0 { 1u16 } else { 0u16 };
            let cursor_x = inner_x + indicator_offset + cursor_col as u16;
            let cursor_y = inner_y + rows_before as u16 + cursor_line as u16;
            Some((cursor_x, cursor_y))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Session;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_sidebar_to_buffer(state: &AppState, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            let area = Rect::new(0, 0, width, height);
            let sidebar = Sidebar::new(state);
            frame.render_widget(sidebar, area);
        }).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_contains(buf: &Buffer, text: &str) -> bool {
        let content: String = (0..buf.area().height)
            .flat_map(|y| {
                (0..buf.area().width)
                    .map(move |x| buf[(x, y)].symbol().to_string())
            })
            .collect();
        content.contains(text)
    }

    #[test]
    fn test_sidebar_title_rendered() {
        let state = AppState::default();
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        // Default workspace name is shown as title
        assert!(buffer_contains(&buf, "Default"));
    }

    #[test]
    fn test_sidebar_title_is_purple() {
        let state = AppState::default();
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        // Title starts at x=2, y=1 (inside border + padding)
        let cell = &buf[(2, 1)];
        assert_eq!(cell.fg, PURPLE, "Title should be purple");
    }

    #[test]
    fn test_sidebar_focused_border_color() {
        let state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        // Top-left corner
        let cell = &buf[(0, 0)];
        assert_eq!(cell.fg, FOCUSED_BORDER, "Focused border should be color 99 (bright purple)");
    }

    #[test]
    fn test_sidebar_unfocused_border_is_dark_grey() {
        let state = AppState {
            focus: Focus::Terminal,
            ..Default::default()
        };
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        // Top-left corner
        let cell = &buf[(0, 0)];
        assert_eq!(cell.fg, DARK_GREY, "Unfocused border should be dark grey");
    }

    #[test]
    fn test_sidebar_welcome_state_shown_when_no_sessions() {
        let state = AppState::default();
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        assert!(buffer_contains(&buf, "Welcome"));
        assert!(buffer_contains(&buf, "Press"));
    }

    #[test]
    fn test_sidebar_welcome_shows_n_when_focused() {
        let state = AppState {
            focus: Focus::Sidebar,
            ..Default::default()
        };
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        // Should show 'n' not 'ctrl+n'
        assert!(buffer_contains(&buf, "Press"));
        // The keybinding should just be 'n' when sidebar is focused
        assert!(buffer_contains(&buf, " n"));
    }

    #[test]
    fn test_sidebar_session_list_rendered() {
        let state = AppState::with_sessions(vec![
            Session::new("session1"),
            Session::new("session2"),
        ]);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        assert!(buffer_contains(&buf, "session1"));
        assert!(buffer_contains(&buf, "session2"));
    }

    #[test]
    fn test_sidebar_selected_session_has_grey_bg() {
        let mut state = AppState::with_sessions(vec![
            Session::new("selected"),
        ]);
        state.selected_index = 0;
        state.focus = Focus::Sidebar;
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);

        // Find the 's' of 'selected' and check its background
        // Session list starts at y=2 (after border and title), x=2 (after border + padding)
        let cell = &buf[(2, 2)];
        assert_eq!(cell.bg, DARK_GREY, "Selected session should have grey background");
    }

    #[test]
    fn test_sidebar_session_names_are_white() {
        let state = AppState::with_sessions(vec![
            Session::new("test"),
        ]);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        // Find the 't' of 'test' at x=2 (after border + padding)
        let cell = &buf[(2, 2)];
        assert_eq!(cell.fg, WHITE, "Session name should be white");
    }

    #[test]
    fn test_wrap_session_name_short() {
        let wrapped = wrap_session_name("short", CONTENT_WIDTH);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].0, "short");
        assert!(!wrapped[0].1); // Not a continuation
        assert!(wrapped[0].2);  // Is last line
    }

    #[test]
    fn test_wrap_session_name_exact_width() {
        let name = "a".repeat(CONTENT_WIDTH);
        let wrapped = wrap_session_name(&name, CONTENT_WIDTH);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].0, name);
    }

    #[test]
    fn test_wrap_session_name_long() {
        // Create a name that's longer than CONTENT_WIDTH
        let name = "a".repeat(CONTENT_WIDTH + 10);
        let wrapped = wrap_session_name(&name, CONTENT_WIDTH);

        assert!(wrapped.len() > 1);
        // First line is not a continuation
        assert!(!wrapped[0].1);
        // Last line is marked as last
        assert!(wrapped.last().unwrap().2);
        // Middle lines are continuations
        if wrapped.len() > 2 {
            assert!(wrapped[1].1);
        }
    }

    #[test]
    fn test_wrap_session_name_continuation_indicators() {
        // Verify continuation lines have correct markers
        let name = "a".repeat(CONTENT_WIDTH * 3);
        let wrapped = wrap_session_name(&name, CONTENT_WIDTH);

        // Should have 3+ lines
        assert!(wrapped.len() >= 3);

        // Middle line should be continuation but not last
        assert!(wrapped[1].1); // is_continuation
        assert!(!wrapped[1].2); // is_last_line

        // Last line should be continuation and last
        let last = wrapped.last().unwrap();
        assert!(last.1); // is_continuation
        assert!(last.2); // is_last_line
    }

    #[test]
    fn test_session_row_count() {
        assert_eq!(session_row_count("short", CONTENT_WIDTH), 1);
        assert_eq!(session_row_count(&"a".repeat(CONTENT_WIDTH), CONTENT_WIDTH), 1);
        assert_eq!(session_row_count(&"a".repeat(CONTENT_WIDTH + 1), CONTENT_WIDTH), 2);
    }

    #[test]
    fn test_truncation_indicator_shown_when_overflow() {
        // Create more sessions than can fit
        let sessions: Vec<Session> = (0..50)
            .map(|i| Session::new(format!("session{}", i)))
            .collect();
        let state = AppState::with_sessions(sessions);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 10);
        assert!(buffer_contains(&buf, "..."));
    }

    #[test]
    fn test_truncation_indicator_is_dark_grey() {
        let sessions: Vec<Session> = (0..50)
            .map(|i| Session::new(format!("session{}", i)))
            .collect();
        let state = AppState::with_sessions(sessions);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 10);

        // Find the "..." indicator
        for y in 0..buf.area().height {
            for x in 0..buf.area().width.saturating_sub(2) {
                let cell = &buf[(x, y)];
                if cell.symbol() == "." {
                    let next = &buf[(x + 1, y)];
                    let next2 = &buf[(x + 2, y)];
                    if next.symbol() == "." && next2.symbol() == "." {
                        assert_eq!(cell.fg, DARK_GREY, "Truncation indicator should be dark grey");
                        return;
                    }
                }
            }
        }
    }

    #[test]
    fn test_continuation_indicators_are_dark_grey() {
        // Create a session with a very long name
        let long_name = "a".repeat(CONTENT_WIDTH * 2);
        let state = AppState::with_sessions(vec![Session::new(long_name)]);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);

        // Find the continuation indicator (│ or └)
        for y in 0..buf.area().height {
            let cell = &buf[(1, y)];
            if cell.symbol() == "│" || cell.symbol() == "└" {
                assert_eq!(cell.fg, DARK_GREY, "Continuation indicator should be dark grey");
                return;
            }
        }
    }

    #[test]
    fn test_selection_highlight_fills_row() {
        let mut state = AppState::with_sessions(vec![Session::new("test")]);
        state.selected_index = 0;
        state.focus = Focus::Sidebar;
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);

        // Check that the row from first letter to right before the right border has dark purple background
        // Per spec: highlight starts at first letter and stops right before the right sidebar border
        // Layout: x=0 border, x=1 padding, x=2-25 content, x=26 padding, x=27 border
        // Row 2 is where the session is (after border and title)
        let y = 2;
        // Content starts at x=2 (after border + padding) and goes through x=25 (CONTENT_WIDTH chars)
        // That's 2..26 exclusive, which covers x=2 through x=25
        for x in 2..2 + CONTENT_WIDTH as u16 {
            let cell = &buf[(x, y)];
            assert_eq!(cell.bg, DARK_GREY, "Selection highlight should fill the row at x={}", x);
        }
        // Left padding area (x=1) should NOT have background highlight
        let left_padding_cell = &buf[(1, y)];
        assert_ne!(left_padding_cell.bg, DARK_GREY, "Left padding area should not have selection highlight");
        // Right padding area (x=26) should NOT have background highlight
        let right_padding_cell = &buf[(SIDEBAR_WIDTH - 2, y)];
        assert_ne!(right_padding_cell.bg, DARK_GREY, "Right padding area should not have selection highlight");
    }

    #[test]
    fn test_get_sidebar_cursor_position_drafting() {
        use crate::state::{DraftingState, SessionType};

        let mut state = AppState::default();
        let mut draft = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        draft.insert_char('a');
        draft.insert_char('b');
        draft.insert_char('c');
        state.mode = AppMode::Drafting(draft);

        let area = Rect::new(0, 0, SIDEBAR_WIDTH, 24);
        let cursor = get_sidebar_cursor_position(&state, area);

        assert!(cursor.is_some());
        let (x, _y) = cursor.unwrap();
        // Cursor should be at position after 'abc': 1 (border) + 1 (padding) + 3 (cursor_position) = 5
        assert_eq!(x, 1 + 1 + 3); // border + padding + cursor_position
    }

    #[test]
    fn test_draft_wraps_while_typing() {
        use crate::state::{DraftingState, SessionType};

        // Type a name longer than CONTENT_WIDTH (24 chars)
        let long_name = "abcdefghijklmnopqrstuvwxyz"; // 26 chars
        let mut state = AppState::default();
        let mut draft = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        for c in long_name.chars() {
            draft.insert_char(c);
        }
        state.mode = AppMode::Drafting(draft);

        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);

        // First 24 chars on row 2, continuation on row 3.
        // Layout: border at x=0, padding at x=1, content starts at x=2.
        assert!(buffer_contains(&buf, "abcdefghijklmnopqrstuvwx"), "First line should contain first 24 chars");
        assert!(buffer_contains(&buf, "yz"), "Continuation line should contain remaining chars");
        // Continuation indicator at content_x=2 on row 3
        let row3_indicator = &buf[(2, 3)];
        assert_eq!(row3_indicator.symbol(), "└", "Continuation indicator should be └ at x=2");
    }

    #[test]
    fn test_draft_cursor_wraps_correctly() {
        use crate::state::{DraftingState, SessionType};

        // Type exactly CONTENT_WIDTH + 1 chars so name wraps
        let name: String = "a".repeat(CONTENT_WIDTH + 1); // 25 chars
        let mut state = AppState::default();
        let mut draft = DraftingState::new(SessionType::Terminal, Focus::Sidebar);
        for c in name.chars() {
            draft.insert_char(c);
        }
        // cursor_position is now 25 (end of name)
        state.mode = AppMode::Drafting(draft);

        let area = Rect::new(0, 0, SIDEBAR_WIDTH, 24);
        let cursor = get_sidebar_cursor_position(&state, area);

        assert!(cursor.is_some());
        let (cursor_x, cursor_y) = cursor.unwrap();
        // Cursor should be on line 1 (the continuation line), col 1 (after 1-char content)
        // cursor_y = area.y + 2 (border+title) + 1 (line 1) = 3
        assert_eq!(cursor_y, area.y + 3, "Cursor should be on the second line (wrapping)");
        // cursor_x = inner_x (2) + indicator_offset (1) + col (1) = 4
        assert_eq!(cursor_x, 1 + 1 + 1 + 1, "Cursor x should account for border, padding, indicator, and col"); // x=4
    }

    #[test]
    fn test_rename_wraps_while_typing() {
        use crate::state::{RenamingState, Session};

        // Session with short name, rename it with a long name
        let mut state = AppState::with_sessions(vec![Session::new("short")]);
        state.selected_index = 0;
        let long_name = "averylongsessionnamethatiswrapped"; // >24 chars
        let mut rename = RenamingState::new(0, long_name, Focus::Sidebar);
        // Move cursor to end
        for _ in 0..long_name.len() {
            rename.move_cursor_right();
        }
        state.mode = AppMode::Renaming(rename);

        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);

        // The first 24 chars should appear on row 2
        assert!(buffer_contains(&buf, &long_name[..24]), "First 24 chars should appear on row 2");
        // The remaining chars should appear on row 3 with a continuation indicator
        assert!(buffer_contains(&buf, &long_name[24..]), "Remaining chars should appear on row 3");
    }

    #[test]
    fn test_cursor_line_col_no_wrap() {
        let wrapped = wrap_session_name("abc", CONTENT_WIDTH);
        assert_eq!(cursor_line_col(&wrapped, 0), (0, 0));
        assert_eq!(cursor_line_col(&wrapped, 2), (0, 2));
        assert_eq!(cursor_line_col(&wrapped, 3), (0, 3));
    }

    #[test]
    fn test_cursor_line_col_with_wrap() {
        let name = "a".repeat(CONTENT_WIDTH + 5); // 29 chars
        let wrapped = wrap_session_name(&name, CONTENT_WIDTH);
        // First 24 chars on line 0, next 5 (23-max continuation) on line 1
        assert_eq!(cursor_line_col(&wrapped, 0), (0, 0));
        assert_eq!(cursor_line_col(&wrapped, 23), (0, 23));
        assert_eq!(cursor_line_col(&wrapped, 24), (0, 24)); // end of line 0
        assert_eq!(cursor_line_col(&wrapped, 25), (1, 1));
        assert_eq!(cursor_line_col(&wrapped, 29), (1, 5)); // end of line 1
    }

    #[test]
    fn test_sidebar_width_constant() {
        assert_eq!(SIDEBAR_WIDTH, 28);
    }

    #[test]
    fn test_content_width_constant() {
        // CONTENT_WIDTH = SIDEBAR_WIDTH (28) - 2 (borders) - 2 (padding) = 24
        assert_eq!(CONTENT_WIDTH, 24);
    }

    #[test]
    fn test_empty_session_name_wrapping() {
        let wrapped = wrap_session_name("", CONTENT_WIDTH);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].0, "");
    }

    #[test]
    fn test_no_sessions_shows_welcome() {
        let state = AppState::default();
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        assert!(buffer_contains(&buf, "Welcome"));
    }

    #[test]
    fn test_sessions_hides_welcome() {
        let state = AppState::with_sessions(vec![Session::new("test")]);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);
        assert!(!buffer_contains(&buf, "Welcome"));
    }

    #[test]
    fn test_sidebar_has_padding_between_content_and_border() {
        // Per spec: The sidebar should have one char of padding on the left and right
        // between the session names and the sidebar border.
        let state = AppState::with_sessions(vec![Session::new("test")]);
        let buf = render_sidebar_to_buffer(&state, SIDEBAR_WIDTH, 24);

        // Row 2 is where session names appear (after border row 0 and title row 1)
        let y = 2;

        // x=0 is the left border
        let border_cell = &buf[(0, y)];
        assert!(
            border_cell.symbol() == "│",
            "Position 0 should be border, got: '{}'",
            border_cell.symbol()
        );

        // x=1 should be padding (empty/space)
        let padding_cell = &buf[(1, y)];
        assert!(
            padding_cell.symbol() == " ",
            "Position 1 should be padding (space), got: '{}'",
            padding_cell.symbol()
        );

        // x=2 should be where the session name starts (the 't' of 'test')
        let content_cell = &buf[(2, y)];
        assert_eq!(
            content_cell.symbol(),
            "t",
            "Position 2 should be first letter of session name"
        );
    }
}
