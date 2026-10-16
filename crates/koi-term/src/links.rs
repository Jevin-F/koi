//! Web links in terminal output: hyperlinks set by programs (OSC 8) and
//! addresses written in the text, which may wrap across rows.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

/// The web address at `point`, if any. Only `http` and `https` addresses
/// count, so clicking output cannot open other kinds of links.
pub fn link_at<T: EventListener>(term: &Term<T>, point: Point) -> Option<String> {
    let grid = term.grid();
    if let Some(link) = grid[point].hyperlink() {
        return is_web(link.uri()).then(|| link.uri().to_owned());
    }
    let last = Column(grid.columns() - 1);
    let wraps = |line: Line| grid[line][last].flags.contains(Flags::WRAPLINE);
    let mut line = point.line;
    while line > grid.topmost_line() && wraps(line - 1) {
        line -= 1;
    }
    // The rows of the logical line, as text, noting where `point` falls.
    let mut text = String::new();
    let mut at = None;
    loop {
        for column in 0..grid.columns() {
            let cell = &grid[line][Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            if line == point.line && column == point.column.0 {
                at = Some(text.len());
            }
            text.push(cell.c);
        }
        if line >= grid.bottommost_line() || !wraps(line) {
            break;
        }
        line += 1;
    }
    url_at(&text, at?).map(str::to_owned)
}

fn is_web(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

/// The address in `text` covering byte `at`. An address ends at a space,
/// a character not allowed in addresses, or a non-ASCII character such as
/// CJK punctuation; trailing punctuation is left out.
fn url_at(text: &str, at: usize) -> Option<&str> {
    text.match_indices("http").find_map(|(start, _)| {
        let rest = &text[start..];
        if !is_web(rest) {
            return None;
        }
        let end = rest
            .find(|c: char| !c.is_ascii_graphic() || "<>\"'`{}|\\^".contains(c))
            .unwrap_or(rest.len());
        let url = trim_punctuation(&rest[..end]);
        let has_host = url
            .split_once("://")
            .is_some_and(|(_, host)| !host.is_empty());
        (has_host && (start..start + url.len()).contains(&at)).then_some(url)
    })
}

/// `url` without trailing sentence punctuation, or closing brackets that
/// do not close one opened inside it, as in `(see https://a.dev/x)`.
fn trim_punctuation(mut url: &str) -> &str {
    loop {
        let unbalanced = |open: char, close: char| {
            url.ends_with(close) && url.matches(close).count() > url.matches(open).count()
        };
        if url.ends_with(['.', ',', ';', ':', '!', '?'])
            || unbalanced('(', ')')
            || unbalanced('[', ']')
        {
            url = &url[..url.len() - 1];
        } else {
            return url;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridSize;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(output: &str, columns: u16) -> Term<VoidListener> {
        let size = GridSize {
            columns,
            lines: 4,
            cell_width: 8,
            cell_height: 16,
        };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, output.as_bytes());
        term
    }

    fn at(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    #[test]
    fn finds_addresses_in_text() {
        let text = "see https://koi.dev/a_(b) and (http://x.io/y).";
        let index = |part: &str| text.find(part).unwrap();
        assert_eq!(url_at(text, index("koi")), Some("https://koi.dev/a_(b)"));
        assert_eq!(url_at(text, index("x.io")), Some("http://x.io/y"));
        assert_eq!(url_at(text, index("see")), None);
        assert_eq!(
            url_at("访问 https://a.dev/x，然后", 8),
            Some("https://a.dev/x")
        );
        assert_eq!(url_at("https:// nothing", 2), None);
        assert_eq!(url_at("ftp://a.dev", 2), None);
    }

    #[test]
    fn addresses_may_wrap_across_rows() {
        let term = term("go https://koi.dev/long/path ok", 12);
        assert_eq!(
            link_at(&term, at(1, 2)).as_deref(),
            Some("https://koi.dev/long/path")
        );
        assert_eq!(
            link_at(&term, at(0, 5)).as_deref(),
            Some("https://koi.dev/long/path")
        );
        assert_eq!(link_at(&term, at(0, 0)), None);
    }

    #[test]
    fn program_hyperlinks_win_and_must_be_web_links() {
        let term = term(
            "\x1b]8;;https://koi.dev/docs\x1b\\docs\x1b]8;;\x1b\\ \x1b]8;;file:///etc/passwd\x1b\\file\x1b]8;;\x1b\\",
            40,
        );
        assert_eq!(
            link_at(&term, at(0, 1)).as_deref(),
            Some("https://koi.dev/docs")
        );
        assert_eq!(link_at(&term, at(0, 6)), None);
    }

    #[test]
    fn wide_characters_keep_columns_aligned() {
        let term = term("中文 https://a.dev", 30);
        assert_eq!(link_at(&term, at(0, 6)).as_deref(), Some("https://a.dev"));
    }
}
