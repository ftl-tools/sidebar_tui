//! Spec-compliant ANSI 256 colors for the Sidebar TUI.
//!
//! All colors defined here use `Color::Indexed(n)` to specify exact ANSI 256
//! color indices as required by the objectives.md spec.

use ratatui::style::Color;

/// Purple (ANSI 99) - Used for title text, keybindings in hint bar.
/// Changed from 55 (#5f00af, dark purple) to 99 (#875fff) to match the focused border color
/// and try a brighter, more violet tone across all purple text elements.
pub const PURPLE: Color = Color::Indexed(99);

/// White (ANSI 255) - Used for session names, terminal text.
pub const WHITE: Color = Color::Indexed(255);

/// Dark grey (ANSI 238) - Used for unfocused borders, wrap indicators,
/// truncation indicators, hint bar background.
pub const DARK_GREY: Color = Color::Indexed(238);

/// Dark purple (ANSI 54) - Used for selected session background.
pub const DARK_PURPLE: Color = Color::Indexed(54);

/// Dark red (ANSI 88) - Used for important confirmation prompt backgrounds.
pub const DARK_RED: Color = Color::Indexed(88);

/// Focused border (ANSI 99) - Used for focused pane outlines.
/// Changed from 93 (#5f00ff, blue-violet) to 99 (#875fff) which is a softer violet-purple
/// that sits between blue and purple, offering better contrast against dark backgrounds as thin lines.
pub const FOCUSED_BORDER: Color = Color::Indexed(99);

/// Separator (ANSI 242) - Used for the separator in the hint bar.
/// More gray than white for visual distinction.
pub const SEPARATOR: Color = Color::Indexed(242);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purple_is_indexed_99() {
        match PURPLE {
            Color::Indexed(n) => assert_eq!(n, 99),
            _ => panic!("PURPLE should be Color::Indexed"),
        }
    }

    #[test]
    fn test_white_is_indexed_255() {
        match WHITE {
            Color::Indexed(n) => assert_eq!(n, 255),
            _ => panic!("WHITE should be Color::Indexed"),
        }
    }

    #[test]
    fn test_dark_grey_is_indexed_238() {
        match DARK_GREY {
            Color::Indexed(n) => assert_eq!(n, 238),
            _ => panic!("DARK_GREY should be Color::Indexed"),
        }
    }

    #[test]
    fn test_dark_purple_is_indexed_54() {
        match DARK_PURPLE {
            Color::Indexed(n) => assert_eq!(n, 54),
            _ => panic!("DARK_PURPLE should be Color::Indexed"),
        }
    }

    #[test]
    fn test_dark_red_is_indexed_88() {
        match DARK_RED {
            Color::Indexed(n) => assert_eq!(n, 88),
            _ => panic!("DARK_RED should be Color::Indexed"),
        }
    }

    #[test]
    fn test_focused_border_is_indexed_99() {
        match FOCUSED_BORDER {
            Color::Indexed(n) => assert_eq!(n, 99),
            _ => panic!("FOCUSED_BORDER should be Color::Indexed"),
        }
    }

    #[test]
    fn test_separator_is_indexed_242() {
        match SEPARATOR {
            Color::Indexed(n) => assert_eq!(n, 242),
            _ => panic!("SEPARATOR should be Color::Indexed"),
        }
    }

    #[test]
    fn test_all_colors_are_distinct() {
        // FOCUSED_BORDER == PURPLE (both are ANSI 99) so excluded from this check
        let colors = [PURPLE, WHITE, DARK_GREY, DARK_PURPLE, DARK_RED, SEPARATOR];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(
                    colors[i], colors[j],
                    "Colors at index {} and {} should be distinct",
                    i, j
                );
            }
        }
    }

    #[test]
    fn test_colors_can_be_used_in_style() {
        use ratatui::style::Style;

        // Verify colors work in Style contexts
        let title_style = Style::default().fg(PURPLE);
        let selected_style = Style::default().bg(DARK_PURPLE).fg(WHITE);
        let hint_bar_style = Style::default().bg(DARK_GREY).fg(WHITE);
        let important_prompt = Style::default().bg(DARK_RED).fg(WHITE);
        let unfocused_border = Style::default().fg(DARK_GREY);
        let focused_border = Style::default().fg(FOCUSED_BORDER);

        // Just verify they compile and don't panic
        assert_eq!(title_style.fg, Some(PURPLE));
        assert_eq!(selected_style.bg, Some(DARK_PURPLE));
        assert_eq!(hint_bar_style.bg, Some(DARK_GREY));
        assert_eq!(important_prompt.bg, Some(DARK_RED));
        assert_eq!(unfocused_border.fg, Some(DARK_GREY));
        assert_eq!(focused_border.fg, Some(FOCUSED_BORDER));
    }
}
