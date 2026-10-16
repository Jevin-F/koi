//! A one-line summary of what a terminal shows.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;

/// The nearest line above the cursor with readable text, without the
/// borders and padding that TUIs draw around it.
pub fn recent_line<T: EventListener>(term: &Term<T>) -> Option<String> {
    let grid = term.grid();
    let cursor = grid.cursor.point.line.0;
    (0..cursor).rev().find_map(|line| {
        let row = &grid[Line(line)];
        let text: String = (0..term.columns())
            .map(|column| row[Column(column)].c)
            .collect();
        let text = text.trim_matches(|c: char| c.is_whitespace() || is_box_drawing(c));
        text.chars()
            .any(char::is_alphanumeric)
            .then(|| text.to_owned())
    })
}

/// The line at the cursor before Enter is sent to a terminal application.
pub fn submitted_line<T: EventListener>(term: &Term<T>) -> Option<String> {
    let row = &term.grid()[term.grid().cursor.point.line];
    let text: String = (0..term.columns())
        .map(|column| row[Column(column)].c)
        .collect();
    let text = text.trim_matches(|c: char| c.is_whitespace() || is_box_drawing(c));
    (!text.is_empty()).then(|| text.to_owned())
}

fn is_box_drawing(c: char) -> bool {
    ('\u{2500}'..='\u{257f}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridSize;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(output: &str) -> Term<VoidListener> {
        let size = GridSize {
            columns: 30,
            lines: 6,
            cell_width: 8,
            cell_height: 16,
        };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, output.as_bytes());
        term
    }

    #[test]
    fn finds_the_last_line_above_the_cursor() {
        let term = term("first\r\nsecond\r\n\r\n$ ");
        assert_eq!(recent_line(&term).as_deref(), Some("second"));
    }

    #[test]
    fn strips_borders_and_skips_decoration() {
        let term = term("│ Edited main.rs │\r\n╰──────────╯\r\n> ");
        assert_eq!(recent_line(&term).as_deref(), Some("Edited main.rs"));
    }

    #[test]
    fn nothing_above_the_cursor() {
        assert_eq!(recent_line(&term("$ ")), None);
    }

    #[test]
    fn captures_the_submitted_input_line() {
        assert_eq!(
            submitted_line(&term("❯ Review PR 385")),
            Some("❯ Review PR 385".into())
        );
        assert_eq!(submitted_line(&term("")), None);
    }
}
