use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::Context;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
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

pub fn render(frame: &mut Frame) {
    let greeting = Paragraph::new("Hello World! (press Ctrl+Q to quit)");
    frame.render_widget(greeting, frame.area());
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
    fn test_render_shows_hello_world() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        assert!(
            content.contains("Hello World!"),
            "Should contain 'Hello World!', got: {}",
            content
        );
        assert!(
            content.contains("Ctrl+Q"),
            "Should contain 'Ctrl+Q', got: {}",
            content
        );
    }

    #[test]
    fn test_render_fits_in_terminal() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Should not panic with a smaller terminal
        terminal.draw(render).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_to_string(buffer);

        // The text should still appear (though possibly truncated)
        assert!(
            content.contains("Hello"),
            "Should contain at least 'Hello', got: {}",
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
