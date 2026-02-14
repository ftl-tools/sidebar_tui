mod pty;

use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::Context;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(render)?;
        if should_quit()? {
            break;
        }
    }
    Ok(())
}

/// Sidebar width in characters
pub const SIDEBAR_WIDTH: u16 = 20;

pub fn render(frame: &mut Frame) {
    // Create horizontal layout: sidebar (20 chars) + terminal view (rest)
    let chunks = Layout::horizontal([
        Constraint::Length(SIDEBAR_WIDTH),
        Constraint::Fill(1),
    ])
    .split(frame.area());

    render_sidebar(frame, chunks[0]);
    render_terminal_view(frame, chunks[1]);
}

fn render_sidebar(frame: &mut Frame, area: Rect) {
    // Split sidebar into header row and body
    let sidebar_chunks = Layout::vertical([
        Constraint::Length(1), // Header row
        Constraint::Fill(1),   // Body
    ])
    .split(area);

    // Header: blue background, black text, centered
    let header = Paragraph::new("Sidebar TUI")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Black).bg(Color::Blue));
    frame.render_widget(header, sidebar_chunks[0]);

    // Body: lighter background (dark gray), empty for now
    let body = Paragraph::new("")
        .style(Style::default().bg(Color::DarkGray));
    frame.render_widget(body, sidebar_chunks[1]);
}

fn render_terminal_view(frame: &mut Frame, area: Rect) {
    // Terminal view - placeholder for now, will be replaced with actual terminal emulator
    let terminal_placeholder = Paragraph::new("Terminal view (press Ctrl+Q to quit)")
        .style(Style::default().bg(Color::Black).fg(Color::White));
    frame.render_widget(terminal_placeholder, area);
}

fn should_quit() -> Result<bool> {
    if event::poll(Duration::from_millis(250)).context("event poll failed")? {
        if let Event::Key(key) = event::read().context("event read failed")? {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q') {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_sidebar_header_shows_title() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Sidebar TUI"),
            "Should contain 'Sidebar TUI', got: {}",
            content
        );
    }

    #[test]
    fn test_sidebar_header_has_blue_background() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Check that the header row (first row, first 20 columns) has blue background
        for x in 0..SIDEBAR_WIDTH {
            let cell = &buffer[(x, 0)];
            assert_eq!(
                cell.bg,
                Color::Blue,
                "Sidebar header at ({}, 0) should have blue background, got: {:?}",
                x,
                cell.bg
            );
        }
    }

    #[test]
    fn test_sidebar_header_has_black_text() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Find the 'S' in "Sidebar TUI" and check its foreground color
        let header_text = "Sidebar TUI";
        let start_x = (SIDEBAR_WIDTH - header_text.len() as u16) / 2;

        for (i, _) in header_text.chars().enumerate() {
            let cell = &buffer[(start_x + i as u16, 0)];
            assert_eq!(
                cell.fg,
                Color::Black,
                "Sidebar header text at ({}, 0) should have black foreground, got: {:?}",
                start_x + i as u16,
                cell.fg
            );
        }
    }

    #[test]
    fn test_sidebar_header_is_centered() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Extract first row within sidebar width
        let mut header_content = String::new();
        for x in 0..SIDEBAR_WIDTH {
            let cell = &buffer[(x, 0)];
            header_content.push_str(cell.symbol());
        }

        // "Sidebar TUI" is 11 chars, sidebar is 20, so 9 spaces total
        // Ratatui centers with floor(remaining/2) on left, so we get 4 left + 5 right
        // Verify the text is roughly centered (within 1 char tolerance)
        let trimmed = header_content.trim();
        assert_eq!(
            trimmed, "Sidebar TUI",
            "Header should contain 'Sidebar TUI'"
        );

        // Count leading spaces
        let leading_spaces = header_content.len() - header_content.trim_start().len();
        // Should be approximately centered: 9 total spaces / 2 = 4-5 on each side
        assert!(
            leading_spaces >= 4 && leading_spaces <= 5,
            "Header should have 4-5 leading spaces for centering, got: {}",
            leading_spaces
        );
    }

    #[test]
    fn test_sidebar_body_has_lighter_background() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Check that the sidebar body (row 1+, first 20 columns) has dark gray background
        for y in 1..24u16 {
            for x in 0..SIDEBAR_WIDTH {
                let cell = &buffer[(x, y)];
                assert_eq!(
                    cell.bg,
                    Color::DarkGray,
                    "Sidebar body at ({}, {}) should have dark gray background, got: {:?}",
                    x,
                    y,
                    cell.bg
                );
            }
        }
    }

    #[test]
    fn test_sidebar_is_20_chars_wide() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // The sidebar should be exactly 20 characters wide
        // Check that column 19 (last sidebar column) has sidebar styling
        // and column 20 (first terminal column) has terminal styling
        assert_eq!(
            buffer[(19, 0)].bg,
            Color::Blue,
            "Column 19 (last sidebar header) should be blue"
        );
        assert_eq!(
            buffer[(20, 0)].bg,
            Color::Black,
            "Column 20 (first terminal column) should be black"
        );
    }

    #[test]
    fn test_terminal_view_fills_rest() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();

        // Terminal view should start at column 20 and have black background
        for x in 20..80u16 {
            let cell = &buffer[(x, 0)];
            assert_eq!(
                cell.bg,
                Color::Black,
                "Terminal view at ({}, 0) should have black background, got: {:?}",
                x,
                cell.bg
            );
        }
    }

    #[test]
    fn test_terminal_view_shows_placeholder() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Ctrl+Q"),
            "Terminal view should contain 'Ctrl+Q', got: {}",
            content
        );
    }

    #[test]
    fn test_render_fits_in_small_terminal() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Should not panic with a smaller terminal
        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // The sidebar title should still appear
        assert!(
            content.contains("Sidebar TUI"),
            "Should contain 'Sidebar TUI', got: {}",
            content
        );
    }

    fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
        let mut result = String::new();
        for y in 0..buffer.area().height {
            for x in 0..buffer.area().width {
                let cell = &buffer[(x, y)];
                result.push_str(cell.symbol());
            }
            result.push('\n');
        }
        result
    }
}
