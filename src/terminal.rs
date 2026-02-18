//! Terminal emulator using vt100 for parsing escape sequences.
//!
//! This module wraps vt100::Parser to provide terminal emulation
//! and rendering to ratatui widgets.

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
/// Uses vt100's built-in scrollback buffer for history.
pub struct Terminal {
    parser: vt100::Parser,
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
            parser: vt100::Parser::new(rows, cols, DEFAULT_HISTORY_SIZE),
            dirty: true, // Start dirty to force initial render
            render_cache: None,
        }
    }

    /// Process raw terminal output (escape sequences, text, etc).
    /// Scroll position is preserved when new output arrives — users can stay scrolled
    /// in history while the terminal continues running. Use reset_scroll() to jump to live.
    pub fn process(&mut self, data: &[u8]) {
        self.parser.process(data);
        if !data.is_empty() {
            self.dirty = true;
        }
    }

    /// Get current scroll offset (0 = live terminal, >0 = scrolled back)
    fn scroll_offset(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Get the current scroll offset (lines scrolled back from the bottom).
    pub fn get_scroll_offset(&self) -> usize {
        self.scroll_offset()
    }

    /// Scroll up (back in history) by the given number of lines.
    /// Returns true if scroll position changed.
    pub fn scroll_up(&mut self, lines: usize) -> bool {
        let current = self.scroll_offset();
        let new_offset = current + lines;
        self.parser.set_scrollback(new_offset);
        let actual = self.scroll_offset();
        if actual != current {
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Scroll down (toward live terminal) by the given number of lines.
    /// Returns true if scroll position changed.
    pub fn scroll_down(&mut self, lines: usize) -> bool {
        let current = self.scroll_offset();
        let new_offset = current.saturating_sub(lines);
        self.parser.set_scrollback(new_offset);
        let actual = self.scroll_offset();
        if actual != current {
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Check if we're currently scrolled back in history.
    #[allow(dead_code)]
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset() > 0
    }

    /// Reset scroll to show live terminal output.
    pub fn reset_scroll(&mut self) {
        self.parser.set_scrollback(0);
        self.dirty = true;
    }

    /// Returns true if the terminal is currently in alternate screen mode.
    /// Alternate screen is used by full-screen apps like vim, less, and htop.
    pub fn is_alt_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Resize the terminal to new dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) {
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
                && cache.scroll_offset == self.scroll_offset()
                && cache.area_width == area.width
                && cache.area_height == area.height
        } else {
            false
        }
    }

    /// Build the lines for rendering. This is the expensive operation that
    /// iterates over all cells and creates Span objects.
    ///
    /// vt100 handles scrollback internally - when set_scrollback() is called,
    /// the cell() method automatically returns cells from the scrollback buffer.
    fn build_lines(&self, area: Rect) -> Vec<Line<'static>> {
        let screen = self.parser.screen();
        let mut lines = Vec::with_capacity(area.height as usize);

        for display_row in 0..area.height {
            let mut spans = Vec::new();

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
                scroll_offset: self.scroll_offset(),
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
    fn test_scroll_up_and_down() {
        // Test basic scroll functionality using vt100's native scrollback
        let mut term = Terminal::new(5, 20);

        // Generate enough content to create scrollback
        for i in 0..30 {
            term.process(format!("Line {}\r\n", i).as_bytes());
        }

        // Should start at scroll offset 0
        assert_eq!(term.scroll_offset(), 0);

        // Scroll up
        term.scroll_up(5);
        assert!(term.scroll_offset() > 0, "scroll_up should increase offset");

        // Scroll down
        let before = term.scroll_offset();
        term.scroll_down(2);
        assert!(term.scroll_offset() < before, "scroll_down should decrease offset");
    }

    #[test]
    fn test_process_preserves_scroll_offset() {
        // Scroll position is preserved when new output arrives so users can stay
        // scrolled in history while the terminal continues running.
        let mut term = Terminal::new(5, 20);

        // Generate scrollback content
        for i in 0..30 {
            term.process(format!("Line {}\r\n", i).as_bytes());
        }

        // Scroll up
        term.scroll_up(5);
        let offset_before = term.scroll_offset();
        assert!(offset_before > 0);

        // New output should NOT reset scroll (user stays in history view)
        term.process(b"new data");
        assert!(term.scroll_offset() > 0, "scroll position should be preserved when new output arrives");
    }

    #[test]
    fn test_reset_scroll_goes_to_bottom() {
        let mut term = Terminal::new(5, 20);
        for i in 0..30 {
            term.process(format!("Line {}\r\n", i).as_bytes());
        }
        term.scroll_up(5);
        assert!(term.scroll_offset() > 0);
        term.reset_scroll();
        assert_eq!(term.scroll_offset(), 0);
    }

    #[test]
    fn test_is_alt_screen_false_by_default() {
        let term = Terminal::new(24, 80);
        assert!(!term.is_alt_screen(), "terminal should not be in alt screen by default");
    }

    #[test]
    fn test_is_alt_screen_true_when_alt_screen_entered() {
        let mut term = Terminal::new(24, 80);
        // Enter alternate screen via escape sequence
        term.process(b"\x1b[?1049h");
        assert!(term.is_alt_screen(), "terminal should be in alt screen after \\e[?1049h");
        // Exit alternate screen
        term.process(b"\x1b[?1049l");
        assert!(!term.is_alt_screen(), "terminal should exit alt screen after \\e[?1049l");
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
