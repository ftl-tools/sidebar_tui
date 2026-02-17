//! Terminal emulator using vt100 for parsing escape sequences.
//!
//! This module wraps vt100::Parser to provide terminal emulation
//! and rendering to ratatui widgets.

use std::borrow::Cow;
use std::collections::VecDeque;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Default number of lines to keep in scrollback history.
const DEFAULT_HISTORY_SIZE: usize = 10000;

/// Static space string to avoid per-cell allocation when rendering empty cells.
/// Using &'static str with Span::styled avoids String allocation entirely.
const SPACE: &str = " ";

/// Default style for empty cells: white foreground for visibility.
/// Pre-computed to avoid recreating on every cell render.
fn default_empty_style() -> Style {
    Style::default().fg(Color::Indexed(255))
}

/// A line in the scrollback history with cell data and styles.
/// Uses Cow<'static, str> to avoid allocations for static space strings.
#[derive(Clone)]
struct HistoryLine {
    cells: Vec<(Cow<'static, str>, Style)>,
}

/// Cached render state for incremental rendering.
/// Stores the previously rendered lines to avoid rebuilding when content hasn't changed.
#[derive(Clone)]
struct RenderCache {
    /// Cached Line objects from last render
    lines: Vec<Line<'static>>,
    /// Scroll offset when cache was built
    scroll_offset: usize,
    /// Terminal dimensions when cache was built
    area_width: u16,
    area_height: u16,
}

/// Terminal emulator that parses escape sequences and maintains screen state.
pub struct Terminal {
    parser: vt100::Parser,
    /// Scroll offset from bottom (0 = showing live terminal, >0 = scrolled back in history)
    scroll_offset: usize,
    /// Scrollback history buffer (oldest lines first)
    history: VecDeque<HistoryLine>,
    /// Maximum history size
    max_history: usize,
    /// Whether content has changed since last render (dirty flag)
    dirty: bool,
    /// Cached render output for incremental rendering
    render_cache: Option<RenderCache>,
}

impl Terminal {
    /// Create a new terminal emulator with the given dimensions.
    ///
    /// # Arguments
    /// * `rows` - Number of rows
    /// * `cols` - Number of columns
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 0),
            scroll_offset: 0,
            history: VecDeque::new(),
            max_history: DEFAULT_HISTORY_SIZE,
            dirty: true, // Start dirty to force initial render
            render_cache: None,
        }
    }

    /// Capture the current screen content and save to history.
    ///
    /// This is called on-demand (before scrolling or on resize) rather than
    /// on every process() call, significantly improving performance during
    /// high-throughput operations like paste.
    fn capture_screen_to_history(&mut self) {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();

        for row in 0..rows {
            let mut cells = Vec::with_capacity(cols as usize);
            for col in 0..cols {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let contents = cell.contents();
                    let style = cell_to_style(cell);
                    // Use Cow::Borrowed for static space string, Cow::Owned for actual content
                    let text: Cow<'static, str> = if contents.is_empty() {
                        Cow::Borrowed(SPACE)
                    } else {
                        Cow::Owned(contents.to_string())
                    };
                    cells.push((text, style));
                } else {
                    cells.push((Cow::Borrowed(SPACE), default_empty_style()));
                }
            }

            // Only add non-empty lines to history
            let is_empty = cells.iter().all(|(s, _)| s.trim().is_empty());
            if !is_empty {
                self.history.push_back(HistoryLine { cells });

                // Trim history if it exceeds max size - O(1) with VecDeque
                if self.history.len() > self.max_history {
                    self.history.pop_front();
                }
            }
        }
    }

    /// Process raw terminal output (escape sequences, text, etc).
    /// Resets scroll offset to show live output when new data arrives.
    ///
    /// Performance optimization: This does NOT capture the screen to history
    /// on every call. History is captured on-demand before scrolling and on
    /// resize. This significantly improves performance during paste operations
    /// where many data chunks arrive rapidly.
    pub fn process(&mut self, data: &[u8]) {
        self.parser.process(data);

        // Reset scroll to bottom when new output arrives (show live terminal)
        // Mark as dirty since content may have changed
        if !data.is_empty() {
            self.dirty = true;
            self.scroll_offset = 0;
        }
    }

    /// Scroll up (back in history) by the given number of lines.
    /// Returns true if scroll position changed.
    ///
    /// Captures current screen to history before scrolling to ensure
    /// the user can see what was on screen in the scrollback.
    pub fn scroll_up(&mut self, lines: usize) -> bool {
        // Capture current screen before scrolling so it appears in history.
        // Only capture if we're not already scrolled (showing live terminal).
        if self.scroll_offset == 0 {
            self.capture_screen_to_history();
        }

        let max_scroll = self.history.len();
        let new_offset = (self.scroll_offset + lines).min(max_scroll);
        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Scroll down (toward live terminal) by the given number of lines.
    /// Returns true if scroll position changed.
    pub fn scroll_down(&mut self, lines: usize) -> bool {
        let new_offset = self.scroll_offset.saturating_sub(lines);
        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Check if we're currently scrolled back in history.
    #[allow(dead_code)]
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Reset scroll to show live terminal output.
    #[allow(dead_code)]
    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }

    /// Resize the terminal to new dimensions.
    ///
    /// Captures current screen to history before resizing to preserve
    /// content that might be lost during the resize.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.capture_screen_to_history();
        self.parser.set_size(rows, cols);
        self.dirty = true;
        // Invalidate cache since dimensions changed
        self.render_cache = None;
    }

    /// Get the current size of the terminal as (rows, cols).
    #[allow(dead_code)]
    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    /// Get the current cursor position as (row, col).
    #[allow(dead_code)]
    pub fn cursor_position(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// Get the plain text contents of the terminal (no formatting).
    #[allow(dead_code)]
    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// Access the underlying vt100 screen for advanced operations.
    #[allow(dead_code)]
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Check if the cache is valid for the current render state.
    fn is_cache_valid(&self, area: Rect) -> bool {
        if let Some(cache) = &self.render_cache {
            !self.dirty
                && cache.scroll_offset == self.scroll_offset
                && cache.area_width == area.width
                && cache.area_height == area.height
        } else {
            false
        }
    }

    /// Build the lines for rendering. This is the expensive operation that
    /// iterates over all cells and creates Span objects.
    fn build_lines(&self, area: Rect) -> Vec<Line<'static>> {
        let screen = self.parser.screen();
        let mut lines = Vec::with_capacity(area.height as usize);

        for display_row in 0..area.height {
            let mut spans = Vec::new();

            if self.scroll_offset > 0 {
                // When scrolled back, show history content
                // scroll_offset=1 means top line of display shows history[len-1]
                // scroll_offset=N means top N lines show history content

                // Calculate which history line to show for this display row
                // History is stored oldest-first, so we want recent history at top when scrolling
                let history_lines_to_show = self.scroll_offset.min(area.height as usize);
                let rows_showing_history = history_lines_to_show as u16;

                if display_row < rows_showing_history {
                    // Show history content
                    // display_row 0 -> history[len - scroll_offset]
                    // display_row 1 -> history[len - scroll_offset + 1]
                    let history_idx = self.history.len().saturating_sub(self.scroll_offset) + display_row as usize;

                    if history_idx < self.history.len() {
                        let history_line = &self.history[history_idx];
                        for (text, style) in &history_line.cells {
                            spans.push(Span::styled(text.clone(), *style));
                        }
                        // Pad to full width using static space
                        while spans.len() < area.width as usize {
                            spans.push(Span::styled(SPACE, default_empty_style()));
                        }
                    } else {
                        // No history for this position, show empty line
                        for _ in 0..area.width {
                            spans.push(Span::styled(SPACE, default_empty_style()));
                        }
                    }
                } else {
                    // Show current screen content (shifted up)
                    let screen_row = display_row - rows_showing_history;
                    for col in 0..area.width {
                        if let Some(cell) = screen.cell(screen_row, col) {
                            if cell.is_wide_continuation() {
                                continue;
                            }
                            let contents = cell.contents();
                            let style = cell_to_style(cell);
                            if contents.is_empty() {
                                spans.push(Span::styled(SPACE, style));
                            } else {
                                spans.push(Span::styled(contents.to_string(), style));
                            }
                        } else {
                            spans.push(Span::styled(SPACE, default_empty_style()));
                        }
                    }
                }
            } else {
                // Not scrolled - show current screen content
                for col in 0..area.width {
                    if let Some(cell) = screen.cell(display_row, col) {
                        // Skip wide continuation cells (second half of wide chars)
                        if cell.is_wide_continuation() {
                            continue;
                        }

                        let contents = cell.contents();
                        let style = cell_to_style(cell);
                        if contents.is_empty() {
                            // Use static space string to avoid allocation
                            spans.push(Span::styled(SPACE, style));
                        } else {
                            // Only allocate for non-empty cells
                            spans.push(Span::styled(contents.to_string(), style));
                        }
                    } else {
                        // Cell doesn't exist, fill with space using explicit white-on-reset style
                        // to ensure consistent colors and avoid terminal state corruption
                        spans.push(Span::styled(SPACE, default_empty_style()));
                    }
                }
            }

            lines.push(Line::from(spans));
        }

        lines
    }

    /// Render the terminal contents to a ratatui frame in the given area.
    /// Handles scroll offset to show scrollback history when scrolled up.
    ///
    /// Uses dirty tracking to avoid rebuilding lines when content hasn't changed.
    /// The cache is invalidated when:
    /// - New output arrives (process() called with data)
    /// - Scroll position changes
    /// - Terminal is resized
    /// - Area dimensions change
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        // Check if we can reuse cached lines
        let lines = if self.is_cache_valid(area) {
            // Reuse cached lines - just clone the Vec (Lines are already 'static)
            self.render_cache.as_ref().unwrap().lines.clone()
        } else {
            // Rebuild lines and update cache
            let lines = self.build_lines(area);
            self.render_cache = Some(RenderCache {
                lines: lines.clone(),
                scroll_offset: self.scroll_offset,
                area_width: area.width,
                area_height: area.height,
            });
            self.dirty = false;
            lines
        };

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, area);
    }

    /// Render the terminal and return the cursor position if visible.
    /// Returns the absolute position of the cursor on the frame.
    pub fn render_with_cursor(&mut self, frame: &mut Frame, area: Rect) -> Option<(u16, u16)> {
        self.render(frame, area);

        let screen = self.parser.screen();
        if !screen.hide_cursor() {
            let (cursor_row, cursor_col) = screen.cursor_position();
            // Return absolute position on frame
            Some((area.x + cursor_col, area.y + cursor_row))
        } else {
            None
        }
    }
}

/// Convert a vt100 cell to a ratatui style.
fn cell_to_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default();

    // Foreground color - use white (255) for default to ensure visibility
    // in all terminal emulators (Apple Terminal, VSCode, etc.)
    style = style.fg(convert_fg_color(cell.fgcolor()));

    // Background color - use Reset for default to be transparent
    style = style.bg(convert_bg_color(cell.bgcolor()));

    // Text modifiers
    let mut modifiers = Modifier::empty();
    if cell.bold() {
        modifiers |= Modifier::BOLD;
    }
    // Note: vt100 0.15 doesn't expose dim/faint attribute
    if cell.italic() {
        modifiers |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifiers |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        modifiers |= Modifier::REVERSED;
    }

    if !modifiers.is_empty() {
        style = style.add_modifier(modifiers);
    }

    style
}

/// Convert a vt100 foreground color to a ratatui color.
///
/// For Default foreground, we use white (ANSI 255) instead of Color::Reset.
/// This ensures text is always visible in terminal emulators that may render
/// Reset as black/dark (like Apple Terminal in TUI mode).
fn convert_fg_color(color: vt100::Color) -> Color {
    match color {
        // Use explicit white for default foreground to ensure visibility
        vt100::Color::Default => Color::Indexed(255),
        vt100::Color::Idx(n) => Color::Indexed(n),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Convert a vt100 background color to a ratatui color.
///
/// For Default background, we use Color::Reset so the background is transparent
/// and inherits from the terminal pane's background.
fn convert_bg_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(n) => Color::Indexed(n),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_new() {
        let term = Terminal::new(24, 80);
        assert_eq!(term.size(), (24, 80));
    }

    #[test]
    fn test_terminal_process_plain_text() {
        let mut term = Terminal::new(24, 80);
        term.process(b"Hello, World!");
        assert!(term.contents().contains("Hello, World!"));
    }

    #[test]
    fn test_terminal_process_newlines() {
        let mut term = Terminal::new(24, 80);
        term.process(b"Line 1\r\nLine 2");
        let contents = term.contents();
        assert!(contents.contains("Line 1"));
        assert!(contents.contains("Line 2"));
    }

    #[test]
    fn test_terminal_cursor_position() {
        let mut term = Terminal::new(24, 80);
        // After writing "ABC", cursor should be at column 3
        term.process(b"ABC");
        let (row, col) = term.cursor_position();
        assert_eq!(row, 0);
        assert_eq!(col, 3);
    }

    #[test]
    fn test_terminal_resize() {
        let mut term = Terminal::new(24, 80);
        term.resize(40, 120);
        assert_eq!(term.size(), (40, 120));
    }

    #[test]
    fn test_terminal_process_colors() {
        let mut term = Terminal::new(24, 80);
        // \x1b[31m sets red foreground
        term.process(b"\x1b[31mRED TEXT\x1b[m");
        let screen = term.screen();
        if let Some(cell) = screen.cell(0, 0) {
            // Index 1 is red in standard terminal colors
            assert_eq!(cell.fgcolor(), vt100::Color::Idx(1));
        }
    }

    #[test]
    fn test_terminal_process_bold() {
        let mut term = Terminal::new(24, 80);
        // \x1b[1m sets bold
        term.process(b"\x1b[1mBOLD\x1b[m");
        let screen = term.screen();
        if let Some(cell) = screen.cell(0, 0) {
            assert!(cell.bold());
        }
    }

    #[test]
    fn test_convert_fg_color_default_is_white() {
        // Default foreground should be white (255) for visibility
        assert_eq!(convert_fg_color(vt100::Color::Default), Color::Indexed(255));
    }

    #[test]
    fn test_convert_bg_color_default_is_reset() {
        // Default background should be Reset (transparent)
        assert_eq!(convert_bg_color(vt100::Color::Default), Color::Reset);
    }

    #[test]
    fn test_convert_fg_color_indexed() {
        assert_eq!(convert_fg_color(vt100::Color::Idx(5)), Color::Indexed(5));
    }

    #[test]
    fn test_convert_bg_color_indexed() {
        assert_eq!(convert_bg_color(vt100::Color::Idx(5)), Color::Indexed(5));
    }

    #[test]
    fn test_convert_fg_color_rgb() {
        assert_eq!(
            convert_fg_color(vt100::Color::Rgb(255, 128, 64)),
            Color::Rgb(255, 128, 64)
        );
    }

    #[test]
    fn test_convert_bg_color_rgb() {
        assert_eq!(
            convert_bg_color(vt100::Color::Rgb(255, 128, 64)),
            Color::Rgb(255, 128, 64)
        );
    }

    #[test]
    fn test_cell_to_style_bold() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b[1mB\x1b[m");
        let screen = term.screen();
        if let Some(cell) = screen.cell(0, 0) {
            let style = cell_to_style(cell);
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn test_cell_to_style_italic() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b[3mI\x1b[m");
        let screen = term.screen();
        if let Some(cell) = screen.cell(0, 0) {
            let style = cell_to_style(cell);
            assert!(style.add_modifier.contains(Modifier::ITALIC));
        }
    }

    #[test]
    fn test_cell_to_style_underline() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b[4mU\x1b[m");
        let screen = term.screen();
        if let Some(cell) = screen.cell(0, 0) {
            let style = cell_to_style(cell);
            assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        }
    }

    // Note: vt100 0.15 doesn't expose dim/faint attribute, so no test for it

    #[test]
    fn test_cell_to_style_inverse() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b[7mR\x1b[m");
        let screen = term.screen();
        if let Some(cell) = screen.cell(0, 0) {
            let style = cell_to_style(cell);
            assert!(style.add_modifier.contains(Modifier::REVERSED));
        }
    }

    #[test]
    fn test_terminal_clear_screen() {
        let mut term = Terminal::new(24, 80);
        term.process(b"Some text here");
        // ESC[2J clears the screen
        term.process(b"\x1b[2J");
        // Contents should be mostly empty now (whitespace)
        assert!(term.contents().trim().is_empty());
    }

    #[test]
    fn test_terminal_cursor_movement() {
        let mut term = Terminal::new(24, 80);
        // Move cursor to row 5, col 10 (1-indexed in escape codes)
        term.process(b"\x1b[5;10H");
        let (row, col) = term.cursor_position();
        assert_eq!(row, 4); // 0-indexed
        assert_eq!(col, 9); // 0-indexed
    }

    #[test]
    fn test_terminal_resize_preserves_content() {
        let mut term = Terminal::new(24, 80);
        term.process(b"Hello, World!");
        term.resize(30, 100);
        // Content should still be present after resize
        assert!(term.contents().contains("Hello"));
        assert_eq!(term.size(), (30, 100));
    }

    #[test]
    fn test_terminal_resize_shrink() {
        let mut term = Terminal::new(24, 80);
        term.resize(12, 40);
        assert_eq!(term.size(), (12, 40));
    }

    #[test]
    fn test_terminal_resize_grow() {
        let mut term = Terminal::new(24, 80);
        term.resize(48, 160);
        assert_eq!(term.size(), (48, 160));
    }

    #[test]
    fn test_process_does_not_capture_history() {
        // Verify that process() does NOT capture to history (for performance).
        // History is only captured on-demand (scroll_up, resize).
        let mut term = Terminal::new(24, 80);

        // Process some data
        term.process(b"Hello, World!");

        // History should still be empty
        assert!(term.history.is_empty(), "process() should not capture to history");
    }

    #[test]
    fn test_scroll_up_captures_history() {
        // Verify that scroll_up() captures the current screen to history.
        let mut term = Terminal::new(24, 80);

        // Process some data
        term.process(b"Hello, World!");

        // History starts empty
        assert!(term.history.is_empty());

        // Scroll up should capture current screen
        term.scroll_up(1);

        // Now history should have content
        assert!(!term.history.is_empty(), "scroll_up should capture to history");
    }

    #[test]
    fn test_resize_captures_history() {
        // Verify that resize() captures the current screen to history.
        let mut term = Terminal::new(24, 80);

        // Process some data
        term.process(b"Hello, World!");

        // History starts empty
        assert!(term.history.is_empty());

        // Resize should capture current screen
        term.resize(30, 100);

        // Now history should have content
        assert!(!term.history.is_empty(), "resize should capture to history");
    }

    #[test]
    fn test_scroll_up_only_captures_once() {
        // Verify that multiple scroll_up calls don't keep re-capturing.
        let mut term = Terminal::new(5, 20);

        // Process some data to have content
        term.process(b"Line 1\r\nLine 2\r\nLine 3");

        // First scroll captures
        term.scroll_up(1);
        let history_len_after_first = term.history.len();

        // Second scroll should NOT re-capture (we're already scrolled)
        term.scroll_up(1);
        let history_len_after_second = term.history.len();

        assert_eq!(
            history_len_after_first, history_len_after_second,
            "scroll_up should only capture when scroll_offset was 0"
        );
    }

    #[test]
    fn test_process_resets_scroll_offset() {
        let mut term = Terminal::new(5, 20);

        // Process some data
        term.process(b"Line 1\r\nLine 2\r\nLine 3");

        // Scroll up
        term.scroll_up(1);
        assert_eq!(term.scroll_offset, 1);

        // New output should reset scroll to bottom
        term.process(b"new data");
        assert_eq!(term.scroll_offset, 0);
    }

    #[test]
    fn test_history_trimming_still_works() {
        // Verify that history still gets trimmed when it exceeds max size.
        let mut term = Terminal::new(5, 20);
        term.max_history = 10; // Small limit for testing

        // Trigger multiple history captures via resize
        for i in 0..15 {
            term.process(format!("line {}\r\n", i).as_bytes());
            term.resize(5, 20); // Each resize captures
        }

        // History should be capped at max_history
        assert!(term.history.len() <= term.max_history);
    }

    #[test]
    fn test_history_uses_cow_borrowed_for_empty_cells() {
        // Verify that empty cells use Cow::Borrowed to avoid allocation.
        let mut term = Terminal::new(5, 20);

        // Process minimal content - most cells will be empty
        term.process(b"A");

        // Capture to history via resize
        term.resize(5, 20);

        // Check that history contains Cow::Borrowed for empty cells
        assert!(!term.history.is_empty());

        let line = &term.history[0];
        // First cell should be "A" (Owned), rest should be spaces (Borrowed)
        let (first_text, _) = &line.cells[0];
        let (second_text, _) = &line.cells[1];

        // First cell is actual content - can be either owned or borrowed
        assert_eq!(first_text.as_ref(), "A");

        // Second cell should be a borrowed space
        assert_eq!(second_text.as_ref(), " ");
        assert!(matches!(second_text, Cow::Borrowed(_)), "Empty cells should use Cow::Borrowed");
    }

    #[test]
    fn test_default_empty_style_returns_white_foreground() {
        // Verify that default_empty_style returns white foreground
        let style = super::default_empty_style();
        assert_eq!(style.fg, Some(Color::Indexed(255)));
    }

    // ========== Dirty Tracking Tests ==========

    #[test]
    fn test_terminal_starts_dirty() {
        // A new terminal should start dirty to force initial render
        let term = Terminal::new(24, 80);
        assert!(term.dirty, "New terminal should start with dirty flag set");
    }

    #[test]
    fn test_process_sets_dirty_flag() {
        let mut term = Terminal::new(24, 80);
        term.dirty = false; // Reset dirty flag

        // Processing data should set dirty flag
        term.process(b"Hello");
        assert!(term.dirty, "Processing data should set dirty flag");
    }

    #[test]
    fn test_process_empty_does_not_set_dirty() {
        let mut term = Terminal::new(24, 80);
        term.dirty = false; // Reset dirty flag

        // Processing empty data should NOT set dirty flag
        term.process(b"");
        assert!(!term.dirty, "Processing empty data should not set dirty flag");
    }

    #[test]
    fn test_scroll_up_sets_dirty() {
        let mut term = Terminal::new(24, 80);
        term.dirty = false;

        // Add some content to make scrolling possible
        for i in 0..30 {
            term.process(format!("Line {}\r\n", i).as_bytes());
        }
        term.dirty = false; // Reset after processing

        // Scrolling should set dirty
        term.scroll_up(5);
        assert!(term.dirty, "scroll_up should set dirty flag when scroll changes");
    }

    #[test]
    fn test_scroll_down_sets_dirty() {
        let mut term = Terminal::new(24, 80);

        // Add content and scroll up first
        for i in 0..30 {
            term.process(format!("Line {}\r\n", i).as_bytes());
        }
        term.scroll_up(10);
        term.dirty = false; // Reset

        // Scrolling down should set dirty
        term.scroll_down(3);
        assert!(term.dirty, "scroll_down should set dirty flag when scroll changes");
    }

    #[test]
    fn test_resize_sets_dirty_and_invalidates_cache() {
        let mut term = Terminal::new(24, 80);
        term.dirty = false;
        term.render_cache = Some(RenderCache {
            lines: vec![],
            scroll_offset: 0,
            area_width: 80,
            area_height: 24,
        });

        term.resize(30, 100);
        assert!(term.dirty, "Resize should set dirty flag");
        assert!(term.render_cache.is_none(), "Resize should invalidate cache");
    }

    #[test]
    fn test_cache_validity_check() {
        let mut term = Terminal::new(24, 80);
        term.dirty = false;
        term.render_cache = Some(RenderCache {
            lines: vec![],
            scroll_offset: 0,
            area_width: 50,
            area_height: 20,
        });

        let area = Rect::new(0, 0, 50, 20);
        assert!(term.is_cache_valid(area), "Cache should be valid when params match");

        // Different width
        let area_diff_width = Rect::new(0, 0, 60, 20);
        assert!(!term.is_cache_valid(area_diff_width), "Cache should be invalid with different width");

        // Different height
        let area_diff_height = Rect::new(0, 0, 50, 25);
        assert!(!term.is_cache_valid(area_diff_height), "Cache should be invalid with different height");

        // Dirty flag set
        term.dirty = true;
        assert!(!term.is_cache_valid(area), "Cache should be invalid when dirty");
    }

    #[test]
    fn test_render_clears_dirty_flag() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal as RatatuiTerminal;

        let mut term = Terminal::new(24, 80);
        assert!(term.dirty, "Should start dirty");

        let backend = TestBackend::new(50, 20);
        let mut ratatui_term = RatatuiTerminal::new(backend).unwrap();

        ratatui_term.draw(|frame| {
            let area = Rect::new(0, 0, 50, 20);
            term.render(frame, area);
        }).unwrap();

        assert!(!term.dirty, "Render should clear dirty flag");
        assert!(term.render_cache.is_some(), "Render should populate cache");
    }

    #[test]
    fn test_render_uses_cache_when_not_dirty() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal as RatatuiTerminal;

        let mut term = Terminal::new(24, 80);
        term.process(b"Test content");

        let backend = TestBackend::new(50, 20);
        let mut ratatui_term = RatatuiTerminal::new(backend).unwrap();

        // First render - builds cache
        ratatui_term.draw(|frame| {
            let area = Rect::new(0, 0, 50, 20);
            term.render(frame, area);
        }).unwrap();

        assert!(!term.dirty);
        let cache_after_first = term.render_cache.clone();

        // Second render - should use cache (dirty is false)
        ratatui_term.draw(|frame| {
            let area = Rect::new(0, 0, 50, 20);
            term.render(frame, area);
        }).unwrap();

        // Cache should remain the same
        assert!(!term.dirty);
        assert_eq!(
            cache_after_first.as_ref().unwrap().scroll_offset,
            term.render_cache.as_ref().unwrap().scroll_offset
        );
    }
}
