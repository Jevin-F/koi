//! Task labels: names the user gives sessions, or derived from the messages
//! they submit. Labels are kept whole up to [`MAX_CHARS`]; the interface
//! cuts them to the space it has.

/// Characters a label keeps.
pub const MAX_CHARS: usize = 100;

/// `text` cut to at most [`MAX_CHARS`] characters, without trailing spaces.
pub fn bounded(text: &str) -> String {
    let label: String = text.chars().take(MAX_CHARS).collect();
    label.trim_end().to_owned()
}

/// First meaningful line of a message.
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
            || is_menu_choice(line)
            || line.chars().count() < min_chars
            || !line.chars().any(char::is_alphabetic)
        {
            return None;
        }
        Some(bounded(line))
    })
}

/// A numbered menu entry such as `2. Skip`, picked rather than typed.
fn is_menu_choice(line: &str) -> bool {
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.len() < line.len() && rest.starts_with(['.', ')']) && rest[1..].starts_with(' ')
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
        for message in [
            "",
            "  ",
            "/help",
            "! ls",
            "$ pwd",
            "1234",
            "ok",
            "2. Skip",
            "› 1) Yes",
        ] {
            assert_eq!(from_message(message), None);
        }
    }

    #[test]
    fn long_names_are_kept_whole_up_to_the_limit() {
        let long = "修".repeat(MAX_CHARS + 20);
        assert_eq!(from_message(&long), Some("修".repeat(MAX_CHARS)));
        let sentence = "Investigate why the release build fails on the signing step";
        assert_eq!(from_message(sentence).as_deref(), Some(sentence));
    }

    #[test]
    fn cut_names_do_not_end_in_a_space() {
        let text = format!("{} tail", "a".repeat(MAX_CHARS - 1));
        assert_eq!(bounded(&text), "a".repeat(MAX_CHARS - 1));
    }
}
