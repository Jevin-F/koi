//! A one-line summary of what a terminal shows.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

/// The nearest line above the cursor with readable text, without the
/// borders and padding that TUIs draw around it.
pub fn recent_line<T: EventListener>(term: &Term<T>) -> Option<String> {
    let grid = term.grid();
    let cursor = grid.cursor.point.line.0;
    (0..cursor).rev().find_map(|line| {
        let text = row_text(term, line);
        text.chars().any(char::is_alphanumeric).then_some(text)
    })
}

/// The text of row `line`, without the borders and padding around it. The
/// second cell of a wide character, such as a Chinese one, is not a space.
fn row_text<T: EventListener>(term: &Term<T>, line: i32) -> String {
    let row = &term.grid()[Line(line)];
    let text: String = (0..term.columns())
        .map(|column| &row[Column(column)])
        .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
        .map(|cell| cell.c)
        .collect();
    text.trim_matches(|c: char| c.is_whitespace() || is_box_drawing(c))
        .to_owned()
}

/// Characters that start a TUI's input line.
const PROMPT_MARKERS: [char; 3] = ['>', '›', '❯'];
/// Most rows a wrapped input is looked for in, above the cursor.
const MAX_INPUT_ROWS: i32 = 12;

/// The input at the cursor before Enter is sent to a terminal application.
/// An input the application wrapped over several rows, under a line that
/// starts with a prompt marker, is joined back into one line.
pub fn submitted_line<T: EventListener>(term: &Term<T>) -> Option<String> {
    let row_text = |line: i32| row_text(term, line);
    let cursor = term.grid().cursor.point.line.0;
    let current = row_text(cursor);
    if current.is_empty() {
        return None;
    }
    if !current.starts_with(PROMPT_MARKERS) {
        let top = term.grid().topmost_line().0.max(cursor - MAX_INPUT_ROWS);
        let mut rows = vec![current.clone()];
        for line in (top..cursor).rev() {
            let text = row_text(line);
            if text.is_empty() {
                break;
            }
            let starts_input = text.starts_with(PROMPT_MARKERS);
            rows.push(text);
            if starts_input {
                rows.reverse();
                return Some(join_wrapped(&rows));
            }
        }
    }
    Some(current)
}

/// Rows of one wrapped input as a line: words split at a row end get their
/// space back, while text without spaces, such as Chinese, is joined as is.
fn join_wrapped(rows: &[String]) -> String {
    let mut line = String::new();
    for row in rows {
        let ascii_seam = line.chars().last().is_some_and(|c| c.is_ascii())
            && row.chars().next().is_some_and(|c| c.is_ascii());
        if ascii_seam {
            line.push(' ');
        }
        line.push_str(row);
    }
    line
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

    #[test]
    fn joins_an_input_wrapped_under_its_prompt_marker() {
        assert_eq!(
            submitted_line(&term("› 修复一下本页面的\r\n  颜色。")).as_deref(),
            Some("› 修复一下本页面的颜色。")
        );
        assert_eq!(
            submitted_line(&term(
                "old output\r\n\r\n› fix the chart\r\n  colors please"
            ))
            .as_deref(),
            Some("› fix the chart colors please")
        );
    }

    #[test]
    fn rows_without_a_prompt_marker_above_stay_one_line() {
        assert_eq!(
            submitted_line(&term("some output\r\nlast row")).as_deref(),
            Some("last row")
        );
    }
}
