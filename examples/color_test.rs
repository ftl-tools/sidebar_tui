//! Visual test for ANSI 256-color support in Apple Terminal
//!
//! Run with: cargo run --example color_test
//!
//! This displays all spec-required colors with their indices for visual verification.

use std::io;

use crossterm::{
    execute,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor, ResetColor},
    terminal::{Clear, ClearType},
};
use ratatui::style::Color as RatatuiColor;

// Spec-required colors
const PURPLE: u8 = 165;       // Title text, keybindings
const WHITE: u8 = 255;        // Session names, terminal text
const DARK_GREY: u8 = 238;    // Unfocused borders, hints bg
const DARK_PURPLE: u8 = 56;   // Selected session background
const DARK_RED: u8 = 88;      // Important prompts background

fn main() -> io::Result<()> {
    let mut stdout = io::stdout();

    execute!(stdout, Clear(ClearType::All))?;

    println!("\n=== ANSI 256-Color Test for Sidebar TUI Spec ===\n");

    // Test each spec color
    let colors = [
        (PURPLE, "Purple (165)", "Title text, keybindings"),
        (WHITE, "White (255)", "Session names, terminal text"),
        (DARK_GREY, "Dark Grey (238)", "Unfocused borders, hint bar bg"),
        (DARK_PURPLE, "Dark Purple (56)", "Selected session background"),
        (DARK_RED, "Dark Red (88)", "Important prompt background"),
    ];

    for (index, name, usage) in colors {
        // Show as foreground
        execute!(
            stdout,
            SetForegroundColor(Color::AnsiValue(index)),
            Print(format!("FG Color {}: {} - {}\n", index, name, usage)),
            ResetColor
        )?;

        // Show as background with contrasting foreground
        let fg = if index == WHITE || index == PURPLE { 0 } else { 255 };
        execute!(
            stdout,
            SetBackgroundColor(Color::AnsiValue(index)),
            SetForegroundColor(Color::AnsiValue(fg)),
            Print(format!("  BG Color {}: {}  ", index, name)),
            ResetColor,
            Print("\n\n")
        )?;
    }

    // Show a mock hint bar
    println!("=== Mock Hint Bar Examples ===\n");

    // Normal hint bar (dark grey bg)
    execute!(
        stdout,
        SetBackgroundColor(Color::AnsiValue(DARK_GREY)),
        SetForegroundColor(Color::AnsiValue(PURPLE)),
        Print("ctrl + n"),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print(" New  "),
        SetForegroundColor(Color::AnsiValue(PURPLE)),
        Print("ctrl + b"),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print(" Focus sidebar"),
        Print("                              "),
        ResetColor,
        Print("\n")
    )?;

    // Important prompt (dark red bg)
    execute!(
        stdout,
        SetBackgroundColor(Color::AnsiValue(DARK_RED)),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("Delete session? "),
        SetForegroundColor(Color::AnsiValue(PURPLE)),
        Print("y"),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print(" Yes  "),
        SetForegroundColor(Color::AnsiValue(PURPLE)),
        Print("n"),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print(" No"),
        Print("                            "),
        ResetColor,
        Print("\n")
    )?;

    // Show a mock sidebar selection
    println!("\n=== Mock Sidebar Selection ===\n");

    execute!(
        stdout,
        SetForegroundColor(Color::AnsiValue(PURPLE)),
        Print("Sidebar TUI\n"),
        ResetColor
    )?;

    // Unselected item
    execute!(
        stdout,
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("Terminal Session\n"),
        ResetColor
    )?;

    // Selected item (dark purple bg)
    execute!(
        stdout,
        SetBackgroundColor(Color::AnsiValue(DARK_PURPLE)),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("Terminal Session"),
        Print("        "),
        ResetColor,
        Print("\n")
    )?;

    // Unselected item
    execute!(
        stdout,
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("Terminal Session\n"),
        ResetColor
    )?;

    // Wrapping indicator (dark grey)
    execute!(
        stdout,
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("Really, really long "),
        Print("\n"),
        SetForegroundColor(Color::AnsiValue(DARK_GREY)),
        Print("\u{2502}"),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("name for this sess"),
        Print("\n"),
        SetForegroundColor(Color::AnsiValue(DARK_GREY)),
        Print("\u{2514}"),
        SetForegroundColor(Color::AnsiValue(WHITE)),
        Print("ion right here"),
        ResetColor,
        Print("\n")
    )?;

    println!("\n=== Ratatui Color::Indexed Verification ===\n");

    // Verify that ratatui's Color::Indexed matches our expectations
    let ratatui_colors: [(RatatuiColor, &str); 5] = [
        (RatatuiColor::Indexed(PURPLE), "Indexed(165) - Purple"),
        (RatatuiColor::Indexed(WHITE), "Indexed(255) - White"),
        (RatatuiColor::Indexed(DARK_GREY), "Indexed(238) - Dark Grey"),
        (RatatuiColor::Indexed(DARK_PURPLE), "Indexed(56) - Dark Purple"),
        (RatatuiColor::Indexed(DARK_RED), "Indexed(88) - Dark Red"),
    ];

    for (color, desc) in ratatui_colors {
        println!("RatatuiColor::{} created successfully", desc);
        match color {
            RatatuiColor::Indexed(n) => println!("  -> Indexed value: {}", n),
            _ => println!("  -> ERROR: Not indexed!"),
        }
    }

    println!("\n=== Test Complete ===");
    println!("If you can see distinct colors above matching the descriptions,");
    println!("ANSI 256-color support is working correctly in your terminal.\n");

    Ok(())
}
