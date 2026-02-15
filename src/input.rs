//! Input handling for converting crossterm keyboard events to PTY bytes.
//!
//! This module converts crossterm KeyEvent to terminal escape sequences
//! that can be sent to the PTY.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Convert a crossterm KeyEvent to bytes suitable for sending to a PTY.
///
/// # Arguments
/// * `key` - The key event from crossterm
///
/// # Returns
/// A Vec<u8> containing the escape sequence or raw bytes to send to the PTY.
/// Returns an empty Vec for keys that should not be forwarded (like modifier-only keys).
pub fn key_to_bytes(key: &KeyEvent) -> Vec<u8> {
    let mods = key.modifiers;
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    let shift = mods.contains(KeyModifiers::SHIFT);

    match key.code {
        // Regular characters
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+letter: send ASCII 1-26
                ctrl_char_to_bytes(c, alt)
            } else if alt {
                // Alt+char: send ESC then the character
                let mut bytes = vec![0x1b];
                let s = c.to_string();
                bytes.extend(s.as_bytes());
                bytes
            } else if shift {
                // Shift+char: send uppercase
                c.to_uppercase().collect::<String>().into_bytes()
            } else {
                // Plain character
                c.to_string().into_bytes()
            }
        }

        // Enter key
        KeyCode::Enter => {
            if alt {
                vec![0x1b, 0x0d]
            } else {
                vec![0x0d]
            }
        }

        // Backspace
        KeyCode::Backspace => {
            if alt {
                vec![0x1b, 0x7f]
            } else {
                vec![0x7f]
            }
        }

        // Tab
        KeyCode::Tab => {
            if shift {
                // Shift+Tab: backtab
                b"\x1b[Z".to_vec()
            } else if alt {
                vec![0x1b, 0x09]
            } else {
                vec![0x09]
            }
        }

        // Escape
        KeyCode::Esc => {
            vec![0x1b]
        }

        // Arrow keys
        KeyCode::Up => encode_arrow(b'A', ctrl, alt, shift),
        KeyCode::Down => encode_arrow(b'B', ctrl, alt, shift),
        KeyCode::Right => encode_arrow(b'C', ctrl, alt, shift),
        KeyCode::Left => encode_arrow(b'D', ctrl, alt, shift),

        // Home/End
        KeyCode::Home => encode_arrow(b'H', ctrl, alt, shift),
        KeyCode::End => encode_arrow(b'F', ctrl, alt, shift),

        // Page Up/Down
        KeyCode::PageUp => encode_tilde(5, ctrl, alt, shift),
        KeyCode::PageDown => encode_tilde(6, ctrl, alt, shift),

        // Insert/Delete
        KeyCode::Insert => encode_tilde(2, ctrl, alt, shift),
        KeyCode::Delete => encode_tilde(3, ctrl, alt, shift),

        // Function keys
        KeyCode::F(n) => encode_function_key(n, ctrl, alt, shift),

        // Modifier-only keys - don't send anything
        KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Modifier(_) => Vec::new(),

        // Null - send null byte
        KeyCode::Null => vec![0x00],

        // Backtab (shift+tab) - already handled by Tab with shift
        KeyCode::BackTab => b"\x1b[Z".to_vec(),

        // Media keys - not typically forwarded
        KeyCode::Media(_) => Vec::new(),
    }
}

/// Convert Ctrl+char to the appropriate control code.
fn ctrl_char_to_bytes(c: char, alt: bool) -> Vec<u8> {
    let ctrl_code = match c.to_ascii_lowercase() {
        // Ctrl+A through Ctrl+Z -> 0x01-0x1a
        c if c.is_ascii_lowercase() => Some((c as u8) - b'a' + 1),
        // Ctrl+@ -> NUL (0x00)
        '@' => Some(0x00),
        // Ctrl+[ -> ESC (0x1b)
        '[' => Some(0x1b),
        // Ctrl+\ -> FS (0x1c)
        '\\' => Some(0x1c),
        // Ctrl+] -> GS (0x1d)
        ']' => Some(0x1d),
        // Ctrl+^ -> RS (0x1e)
        '^' | '6' => Some(0x1e),
        // Ctrl+_ or Ctrl+/ -> US (0x1f)
        '_' | '/' | '-' => Some(0x1f),
        // Ctrl+? -> DEL (0x7f)
        '?' => Some(0x7f),
        // Ctrl+Space -> NUL
        ' ' => Some(0x00),
        // Numbers Ctrl+2 = NUL, Ctrl+3-7 = ESC, FS, GS, RS, US
        '2' => Some(0x00),
        '3' => Some(0x1b),
        '4' => Some(0x1c),
        '5' => Some(0x1d),
        '7' => Some(0x1f),
        '8' => Some(0x7f),
        _ => None,
    };

    match ctrl_code {
        Some(code) => {
            if alt {
                vec![0x1b, code]
            } else {
                vec![code]
            }
        }
        None => {
            // No mapping, just send the character with alt prefix if needed
            if alt {
                let mut bytes = vec![0x1b];
                bytes.extend(c.to_string().as_bytes());
                bytes
            } else {
                c.to_string().into_bytes()
            }
        }
    }
}

/// Encode arrow/home/end keys with optional modifiers.
fn encode_arrow(code: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    let modifier = encode_modifier(ctrl, alt, shift);
    if modifier == 0 {
        // No modifiers: simple CSI sequence
        vec![0x1b, b'[', code]
    } else {
        // With modifiers: CSI 1 ; modifier code
        format!("\x1b[1;{}{}", modifier, code as char).into_bytes()
    }
}

/// Encode tilde-terminated keys (PageUp, PageDown, Insert, Delete).
fn encode_tilde(n: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    let modifier = encode_modifier(ctrl, alt, shift);
    if modifier == 0 {
        format!("\x1b[{}~", n).into_bytes()
    } else {
        format!("\x1b[{};{}~", n, modifier).into_bytes()
    }
}

/// Encode function keys F1-F24.
fn encode_function_key(n: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    let modifier = encode_modifier(ctrl, alt, shift);

    // F1-F4 use SS3 encoding when no modifiers
    if modifier == 0 && (1..=4).contains(&n) {
        let code = match n {
            1 => 'P',
            2 => 'Q',
            3 => 'R',
            4 => 'S',
            _ => unreachable!(),
        };
        return format!("\x1bO{}", code).into_bytes();
    }

    // F1-F4 with modifiers
    if (1..=4).contains(&n) {
        let code = match n {
            1 => 'P',
            2 => 'Q',
            3 => 'R',
            4 => 'S',
            _ => unreachable!(),
        };
        return format!("\x1b[1;{}{}", modifier, code).into_bytes();
    }

    // F5-F24 use CSI number ~
    let code = match n {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        13 => 25,
        14 => 26,
        15 => 28,
        16 => 29,
        17 => 31,
        18 => 32,
        19 => 33,
        20 => 34,
        // F21-F24 are less standardized
        _ => return Vec::new(),
    };

    if modifier == 0 {
        format!("\x1b[{}~", code).into_bytes()
    } else {
        format!("\x1b[{};{}~", code, modifier).into_bytes()
    }
}

/// Encode modifiers as the xterm modifier parameter.
/// Returns 0 if no modifiers, otherwise returns modifier + 1.
fn encode_modifier(ctrl: bool, alt: bool, shift: bool) -> u8 {
    let mut m: u8 = 0;
    if shift {
        m |= 1;
    }
    if alt {
        m |= 2;
    }
    if ctrl {
        m |= 4;
    }
    if m == 0 {
        0
    } else {
        m + 1
    }
}

/// Encode a mouse scroll event as SGR mouse escape sequence.
/// This is used to forward scroll wheel events to the terminal/PTY.
///
/// # Arguments
/// * `scroll_up` - True for scroll up, false for scroll down
/// * `col` - 1-indexed column position
/// * `row` - 1-indexed row position
///
/// # Returns
/// SGR-encoded mouse escape sequence bytes
pub fn encode_mouse_scroll(scroll_up: bool, col: u16, row: u16) -> Vec<u8> {
    // SGR mouse encoding: \x1b[<button;col;rowM
    // Button 64 = scroll up, Button 65 = scroll down
    let button = if scroll_up { 64 } else { 65 };
    format!("\x1b[<{};{};{}M", button, col, row).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_plain_char() {
        let event = key_event(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"a".to_vec());
    }

    #[test]
    fn test_uppercase_char() {
        let event = key_event(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(key_to_bytes(&event), b"A".to_vec());
    }

    #[test]
    fn test_shift_lowercase_becomes_uppercase() {
        let event = key_event(KeyCode::Char('a'), KeyModifiers::SHIFT);
        assert_eq!(key_to_bytes(&event), b"A".to_vec());
    }

    #[test]
    fn test_ctrl_c() {
        let event = key_event(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&event), vec![0x03]);
    }

    #[test]
    fn test_ctrl_a() {
        let event = key_event(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&event), vec![0x01]);
    }

    #[test]
    fn test_ctrl_z() {
        let event = key_event(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&event), vec![0x1a]);
    }

    #[test]
    fn test_alt_a() {
        let event = key_event(KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(key_to_bytes(&event), vec![0x1b, b'a']);
    }

    #[test]
    fn test_enter() {
        let event = key_event(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), vec![0x0d]);
    }

    #[test]
    fn test_backspace() {
        let event = key_event(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), vec![0x7f]);
    }

    #[test]
    fn test_tab() {
        let event = key_event(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), vec![0x09]);
    }

    #[test]
    fn test_shift_tab() {
        let event = key_event(KeyCode::Tab, KeyModifiers::SHIFT);
        assert_eq!(key_to_bytes(&event), b"\x1b[Z".to_vec());
    }

    #[test]
    fn test_backtab() {
        let event = key_event(KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[Z".to_vec());
    }

    #[test]
    fn test_escape() {
        let event = key_event(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), vec![0x1b]);
    }

    #[test]
    fn test_arrow_up() {
        let event = key_event(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[A".to_vec());
    }

    #[test]
    fn test_arrow_down() {
        let event = key_event(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[B".to_vec());
    }

    #[test]
    fn test_arrow_right() {
        let event = key_event(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[C".to_vec());
    }

    #[test]
    fn test_arrow_left() {
        let event = key_event(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[D".to_vec());
    }

    #[test]
    fn test_ctrl_arrow_up() {
        let event = key_event(KeyCode::Up, KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&event), b"\x1b[1;5A".to_vec());
    }

    #[test]
    fn test_home() {
        let event = key_event(KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[H".to_vec());
    }

    #[test]
    fn test_end() {
        let event = key_event(KeyCode::End, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[F".to_vec());
    }

    #[test]
    fn test_page_up() {
        let event = key_event(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[5~".to_vec());
    }

    #[test]
    fn test_page_down() {
        let event = key_event(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[6~".to_vec());
    }

    #[test]
    fn test_insert() {
        let event = key_event(KeyCode::Insert, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[2~".to_vec());
    }

    #[test]
    fn test_delete() {
        let event = key_event(KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[3~".to_vec());
    }

    #[test]
    fn test_f1() {
        let event = key_event(KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1bOP".to_vec());
    }

    #[test]
    fn test_f2() {
        let event = key_event(KeyCode::F(2), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1bOQ".to_vec());
    }

    #[test]
    fn test_f3() {
        let event = key_event(KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1bOR".to_vec());
    }

    #[test]
    fn test_f4() {
        let event = key_event(KeyCode::F(4), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1bOS".to_vec());
    }

    #[test]
    fn test_f5() {
        let event = key_event(KeyCode::F(5), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[15~".to_vec());
    }

    #[test]
    fn test_f12() {
        let event = key_event(KeyCode::F(12), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), b"\x1b[24~".to_vec());
    }

    #[test]
    fn test_ctrl_shift_arrow() {
        let event = key_event(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(key_to_bytes(&event), b"\x1b[1;6A".to_vec());
    }

    #[test]
    fn test_alt_backspace() {
        let event = key_event(KeyCode::Backspace, KeyModifiers::ALT);
        assert_eq!(key_to_bytes(&event), vec![0x1b, 0x7f]);
    }

    #[test]
    fn test_modifier_encode() {
        // No modifiers
        assert_eq!(encode_modifier(false, false, false), 0);
        // Shift only -> 2
        assert_eq!(encode_modifier(false, false, true), 2);
        // Alt only -> 3
        assert_eq!(encode_modifier(false, true, false), 3);
        // Ctrl only -> 5
        assert_eq!(encode_modifier(true, false, false), 5);
        // Ctrl+Shift -> 6
        assert_eq!(encode_modifier(true, false, true), 6);
        // Ctrl+Alt -> 7
        assert_eq!(encode_modifier(true, true, false), 7);
        // Ctrl+Alt+Shift -> 8
        assert_eq!(encode_modifier(true, true, true), 8);
    }

    #[test]
    fn test_modifier_only_keys_are_empty() {
        let event = key_event(KeyCode::CapsLock, KeyModifiers::NONE);
        assert!(key_to_bytes(&event).is_empty());
    }

    #[test]
    fn test_ctrl_space() {
        let event = key_event(KeyCode::Char(' '), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&event), vec![0x00]);
    }

    #[test]
    fn test_unicode_char() {
        let event = key_event(KeyCode::Char('é'), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), "é".as_bytes().to_vec());
    }

    #[test]
    fn test_unicode_cjk_char() {
        let event = key_event(KeyCode::Char('日'), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&event), "日".as_bytes().to_vec());
    }

    #[test]
    fn test_encode_mouse_scroll_up() {
        // Scroll up at column 10, row 5 should produce SGR sequence with button 64
        let result = encode_mouse_scroll(true, 10, 5);
        assert_eq!(result, b"\x1b[<64;10;5M".to_vec());
    }

    #[test]
    fn test_encode_mouse_scroll_down() {
        // Scroll down at column 10, row 5 should produce SGR sequence with button 65
        let result = encode_mouse_scroll(false, 10, 5);
        assert_eq!(result, b"\x1b[<65;10;5M".to_vec());
    }

    #[test]
    fn test_encode_mouse_scroll_at_origin() {
        // Scroll at position (1, 1)
        let result = encode_mouse_scroll(true, 1, 1);
        assert_eq!(result, b"\x1b[<64;1;1M".to_vec());
    }

    #[test]
    fn test_encode_mouse_scroll_large_position() {
        // Scroll at large position (200, 100)
        let result = encode_mouse_scroll(false, 200, 100);
        assert_eq!(result, b"\x1b[<65;200;100M".to_vec());
    }
}
