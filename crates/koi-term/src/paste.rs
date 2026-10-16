//! Encodes pasted text for the terminal.

const BRACKET_START: &str = "\x1b[200~";
const BRACKET_END: &str = "\x1b[201~";

/// Prepares `text` for writing to the PTY.
///
/// Line endings become carriage returns, as if typed. With bracketed paste
/// the text is wrapped in markers so the application can tell it apart from
/// typing; an end marker inside the text is removed so the paste cannot
/// terminate early and inject keystrokes.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!(
            "{BRACKET_START}{}{BRACKET_END}",
            text.replace(BRACKET_END, "")
        )
        .into_bytes()
    } else {
        text.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newlines_become_carriage_returns() {
        assert_eq!(encode_paste("a\nb\r\nc", false), b"a\rb\rc");
    }

    #[test]
    fn bracketed_paste_wraps_text() {
        assert_eq!(encode_paste("hi", true), b"\x1b[200~hi\x1b[201~");
    }

    #[test]
    fn bracketed_paste_strips_embedded_end_marker() {
        assert_eq!(
            encode_paste("a\x1b[201~rm -rf\n", true),
            b"\x1b[200~arm -rf\r\x1b[201~"
        );
    }
}
