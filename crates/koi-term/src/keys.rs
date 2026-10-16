//! Translates key presses into the bytes a terminal application expects.
//!
//! Supports the xterm legacy encoding and the "disambiguate escape codes"
//! level of the kitty keyboard protocol, which agents such as Claude Code and
//! Codex enable to tell apart keys like shift+enter.

use alacritty_terminal::term::TermMode;

use crate::Modifiers;

const ESC: u8 = 0x1b;

/// Encodes a key press. `key` uses GPUI key names (`"enter"`, `"up"`, `"a"`).
/// Returns `None` for keys the terminal has no encoding for.
pub fn encode_key(key: &str, mods: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    let kitty = mode.contains(TermMode::DISAMBIGUATE_ESC_CODES);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let bytes = match key {
        "up" => cursor_key(b'A', mods, app_cursor),
        "down" => cursor_key(b'B', mods, app_cursor),
        "right" => cursor_key(b'C', mods, app_cursor),
        "left" => cursor_key(b'D', mods, app_cursor),
        "home" => cursor_key(b'H', mods, app_cursor),
        "end" => cursor_key(b'F', mods, app_cursor),
        "insert" => tilde_key(2, mods),
        "delete" => tilde_key(3, mods),
        "pageup" => tilde_key(5, mods),
        "pagedown" => tilde_key(6, mods),
        "f1" => ss3_key(b'P', mods),
        "f2" => ss3_key(b'Q', mods),
        "f3" => ss3_key(b'R', mods),
        "f4" => ss3_key(b'S', mods),
        "f5" => tilde_key(15, mods),
        "f6" => tilde_key(17, mods),
        "f7" => tilde_key(18, mods),
        "f8" => tilde_key(19, mods),
        "f9" => tilde_key(20, mods),
        "f10" => tilde_key(21, mods),
        "f11" => tilde_key(23, mods),
        "f12" => tilde_key(24, mods),
        "enter" => control_key(b'\r', mods, kitty),
        "tab" if mods == SHIFT && !kitty => b"\x1b[Z".to_vec(),
        "tab" => control_key(b'\t', mods, kitty),
        "backspace" if mods == CONTROL && !kitty => vec![0x08],
        "backspace" => control_key(0x7f, mods, kitty),
        "escape" if kitty => csi_u(u32::from(ESC), mods),
        "escape" => control_key(ESC, mods, kitty),
        "space" => encode_char(' ', mods, kitty),
        _ => {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => encode_char(c, mods, kitty),
                _ => return None,
            }
        }
    };
    Some(bytes)
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    alt: false,
    control: false,
};

const CONTROL: Modifiers = Modifiers {
    shift: false,
    alt: false,
    control: true,
};

fn csi(body: &str) -> Vec<u8> {
    format!("\x1b[{body}").into_bytes()
}

fn csi_u(code: u32, mods: Modifiers) -> Vec<u8> {
    if mods.any() {
        csi(&format!("{code};{}u", mods.param()))
    } else {
        csi(&format!("{code}u"))
    }
}

fn cursor_key(final_byte: u8, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
    let final_char = char::from(final_byte);
    if mods.any() {
        csi(&format!("1;{}{final_char}", mods.param()))
    } else if app_cursor {
        vec![ESC, b'O', final_byte]
    } else {
        csi(&final_char.to_string())
    }
}

fn ss3_key(final_byte: u8, mods: Modifiers) -> Vec<u8> {
    if mods.any() {
        csi(&format!("1;{}{}", mods.param(), char::from(final_byte)))
    } else {
        vec![ESC, b'O', final_byte]
    }
}

fn tilde_key(number: u8, mods: Modifiers) -> Vec<u8> {
    if mods.any() {
        csi(&format!("{number};{}~", mods.param()))
    } else {
        csi(&format!("{number}~"))
    }
}

/// Enter, tab, backspace and escape: a single control byte in legacy mode.
fn control_key(byte: u8, mods: Modifiers, kitty: bool) -> Vec<u8> {
    if kitty && mods.any() {
        csi_u(u32::from(byte), mods)
    } else if mods.alt {
        vec![ESC, byte]
    } else {
        vec![byte]
    }
}

fn encode_char(c: char, mods: Modifiers, kitty: bool) -> Vec<u8> {
    if kitty && (mods.control || mods.alt) {
        return csi_u(u32::from(c.to_ascii_lowercase()), mods);
    }
    let control_byte = if mods.control { control_byte(c) } else { None };
    let mut bytes = Vec::with_capacity(5);
    if mods.alt {
        bytes.push(ESC);
    }
    match control_byte {
        Some(byte) => bytes.push(byte),
        None => {
            let c = if mods.shift {
                c.to_ascii_uppercase()
            } else {
                c
            };
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        }
    }
    bytes
}

/// The C0 control byte produced by holding control with `c`.
fn control_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' | 'A'..='Z' => Some(c.to_ascii_lowercase() as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(ESC),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        alt: false,
        control: false,
    };
    const ALT: Modifiers = Modifiers {
        shift: false,
        alt: true,
        control: false,
    };

    fn legacy(key: &str, mods: Modifiers) -> Vec<u8> {
        encode_key(key, mods, TermMode::empty()).expect("key has an encoding")
    }

    fn kitty(key: &str, mods: Modifiers) -> Vec<u8> {
        encode_key(key, mods, TermMode::DISAMBIGUATE_ESC_CODES).expect("key has an encoding")
    }

    #[test]
    fn plain_keys() {
        assert_eq!(legacy("enter", NONE), b"\r");
        assert_eq!(legacy("tab", NONE), b"\t");
        assert_eq!(legacy("backspace", NONE), b"\x7f");
        assert_eq!(legacy("escape", NONE), b"\x1b");
        assert_eq!(legacy("space", NONE), b" ");
        assert_eq!(legacy("a", NONE), b"a");
        assert_eq!(legacy("a", SHIFT), b"A");
    }

    #[test]
    fn cursor_keys_follow_application_mode() {
        assert_eq!(legacy("up", NONE), b"\x1b[A");
        assert_eq!(
            encode_key("up", NONE, TermMode::APP_CURSOR).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(legacy("left", SHIFT), b"\x1b[1;2D");
        assert_eq!(legacy("home", NONE), b"\x1b[H");
    }

    #[test]
    fn function_and_editing_keys() {
        assert_eq!(legacy("f1", NONE), b"\x1bOP");
        assert_eq!(legacy("f1", CONTROL), b"\x1b[1;5P");
        assert_eq!(legacy("f5", NONE), b"\x1b[15~");
        assert_eq!(legacy("delete", NONE), b"\x1b[3~");
        assert_eq!(legacy("pageup", ALT), b"\x1b[5;3~");
    }

    #[test]
    fn control_combinations() {
        assert_eq!(legacy("c", CONTROL), [0x03]);
        assert_eq!(legacy("C", CONTROL), [0x03]);
        assert_eq!(legacy("[", CONTROL), [ESC]);
        assert_eq!(legacy("space", CONTROL), [0x00]);
        assert_eq!(legacy("backspace", CONTROL), [0x08]);
        assert_eq!(legacy("tab", SHIFT), b"\x1b[Z");
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(legacy("b", ALT), b"\x1bb");
        assert_eq!(legacy("enter", ALT), b"\x1b\r");
        let ctrl_alt = Modifiers {
            alt: true,
            control: true,
            shift: false,
        };
        assert_eq!(legacy("x", ctrl_alt), [ESC, 0x18]);
    }

    #[test]
    fn kitty_disambiguates_modified_keys() {
        assert_eq!(kitty("enter", NONE), b"\r");
        assert_eq!(kitty("enter", SHIFT), b"\x1b[13;2u");
        assert_eq!(kitty("tab", SHIFT), b"\x1b[9;2u");
        assert_eq!(kitty("escape", NONE), b"\x1b[27u");
        assert_eq!(kitty("c", CONTROL), b"\x1b[99;5u");
        assert_eq!(kitty("a", ALT), b"\x1b[97;3u");
        assert_eq!(kitty("a", SHIFT), b"A");
    }

    #[test]
    fn unknown_keys_have_no_encoding() {
        assert_eq!(encode_key("capslock", NONE, TermMode::empty()), None);
    }
}
