//! Terminal emulator using vt100 for parsing escape sequences.
//!
//! This module wraps vt100::Parser to provide terminal emulation
//! and rendering to ratatui widgets.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Terminal emulator that parses escape sequences and maintains screen state.
pub struct Terminal {
    parser: vt100::Parser,
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
        }
    }

    /// Process raw terminal output (escape sequences, text, etc).
    pub fn process(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    /// Resize the terminal to new dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(rows, cols);
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

    /// Render the terminal contents to a ratatui frame in the given area.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let screen = self.parser.screen();
        let mut lines = Vec::with_capacity(area.height as usize);

        for row in 0..area.height {
            let mut spans = Vec::new();

            for col in 0..area.width {
                if let Some(cell) = screen.cell(row, col) {
                    // Skip wide continuation cells (second half of wide chars)
                    if cell.is_wide_continuation() {
                        continue;
                    }

                    let contents = cell.contents();
                    let text = if contents.is_empty() {
                        " ".to_string()
                    } else {
                        contents.to_string()
                    };

                    let style = cell_to_style(cell);
                    spans.push(Span::styled(text, style));
                } else {
                    // Cell doesn't exist, fill with space
                    spans.push(Span::raw(" "));
                }
            }

            lines.push(Line::from(spans));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, area);
    }

    /// Render the terminal and return the cursor position if visible.
    /// Returns the absolute position of the cursor on the frame.
    pub fn render_with_cursor(&self, frame: &mut Frame, area: Rect) -> Option<(u16, u16)> {
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
}
