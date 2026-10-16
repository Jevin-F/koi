//! Short task labels derived from submitted user messages.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display columns a label may take; wide characters such as CJK count twice.
const MAX_WIDTH: usize = 28;

/// First meaningful line of a message, shortened for tabs and the sidebar.
pub fn from_message(message: &str) -> Option<String> {
    message.lines().find_map(|line| {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = line.trim_start_matches(['>', '›', '❯']).trim_start();
        let min_chars = if line.chars().any(|c| !c.is_ascii() && c.is_alphabetic()) {
            2
        } else {
            4
        };
        if line.starts_with(['/', '!', '$'])
            || line.chars().count() < min_chars
            || !line.chars().any(char::is_alphabetic)
        {
            return None;
        }
        Some(shorten(line))
    })
}

/// Cuts `line` to at most `MAX_WIDTH` columns, ending with `…` when cut.
fn shorten(line: &str) -> String {
    if line.width() <= MAX_WIDTH {
        return line.to_owned();
    }
    let mut width = 0;
    // Strictly below the limit, leaving one column for the ellipsis.
    let label: String = line
        .chars()
        .take_while(|c| {
            width += c.width().unwrap_or(0);
            width < MAX_WIDTH
        })
        .collect();
    format!("{}…", label.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_meaningful_line_becomes_a_short_name() {
        assert_eq!(
            from_message("\n  修复   PR 385 的状态检查  \n详细步骤"),
            Some("修复 PR 385 的状态检查".into())
        );
        assert_eq!(
            from_message("❯ Investigate the test failure"),
            Some("Investigate the test failure".into())
        );
        assert_eq!(
            from_message("/model\nPlease review this patch"),
            Some("Please review this patch".into())
        );
        assert_eq!(from_message("你好"), Some("你好".into()));
    }

    #[test]
    fn commands_and_empty_text_do_not_name_a_session() {
        for message in ["", "  ", "/help", "! ls", "$ pwd", "1234", "ok"] {
            assert_eq!(from_message(message), None);
        }
    }

    #[test]
    fn long_names_are_bounded_by_display_width() {
        let label = from_message(&"修".repeat(60)).unwrap();
        assert_eq!(label, format!("{}…", "修".repeat(13)));
        let label = from_message(&"a".repeat(60)).unwrap();
        assert_eq!(label, format!("{}…", "a".repeat(MAX_WIDTH - 1)));
        assert!(label.width() <= MAX_WIDTH);
    }

    #[test]
    fn names_that_fit_are_kept_whole() {
        let exact = "a".repeat(MAX_WIDTH);
        assert_eq!(from_message(&exact), Some(exact));
    }

    #[test]
    fn cut_names_do_not_end_in_a_space() {
        let label = from_message(&format!("{} tail of the message", "a".repeat(26))).unwrap();
        assert_eq!(label, format!("{}…", "a".repeat(26)));
    }
}
