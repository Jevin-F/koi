//! A drawable snapshot of the visible terminal screen.
//!
//! The grid is reduced to text spans, background fills and a cursor, all in
//! viewport cell coordinates with resolved colors, so a renderer only has to
//! lay out text and fill rectangles.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::Palette;

/// How a span of text is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStyle {
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

/// Consecutive cells on one line that share a style.
///
/// A double-width character always gets a span of its own so the renderer
/// can give it two cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSpan {
    pub line: usize,
    pub column: usize,
    pub text: String,
    /// Number of cells the span covers.
    pub width: usize,
    pub wide: bool,
    pub style: TextStyle,
}

/// A run of cells on one line painted with a non-default background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub line: usize,
    pub column: usize,
    pub width: usize,
    pub color: Rgb,
}

/// Where the cursor is and how to draw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub column: usize,
    /// `Hidden` when the application hid the cursor; the position still tells
    /// an input method where to place its candidate window.
    pub shape: CursorShape,
    pub color: Rgb,
    pub wide: bool,
    /// The character under the cursor, redrawn on top of a block cursor.
    pub text: Option<TextSpan>,
}

/// Everything needed to draw the visible screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub foreground: Rgb,
    pub background: Rgb,
    pub fills: Vec<Fill>,
    pub spans: Vec<TextSpan>,
    /// `None` while the cursor is scrolled out of view.
    pub cursor: Option<Cursor>,
}

impl Screen {
    /// Captures the visible part of `term`.
    pub fn capture<T: EventListener>(term: &Term<T>, palette: &Palette) -> Self {
        let content = term.renderable_content();
        let overrides = content.colors;
        let offset = content.display_offset as i32;
        let background = palette.resolve(Color::Named(NamedColor::Background), overrides);
        let mut screen = Self {
            foreground: palette.resolve(Color::Named(NamedColor::Foreground), overrides),
            background,
            fills: Vec::new(),
            spans: Vec::new(),
            cursor: None,
        };

        for indexed in content.display_iter {
            let cell = indexed.cell;
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let line = (indexed.point.line.0 + offset) as usize;
            let column = indexed.point.column.0;
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            let colors = palette.cell_colors(cell, overrides);
            let selected = content
                .selection
                .is_some_and(|selection| selection.contains(indexed.point));
            let fill = if selected {
                Some(palette.selection)
            } else {
                (colors.background != background).then_some(colors.background)
            };
            if let Some(color) = fill {
                screen.push_fill(line, column, if wide { 2 } else { 1 }, color);
            }
            if !is_blank(cell) {
                screen.push_text(line, column, cell, wide, style(cell, colors.foreground));
            }
        }

        let cursor_line = content.cursor.point.line.0 + offset;
        if (0..term.screen_lines() as i32).contains(&cursor_line) {
            let line = cursor_line as usize;
            let column = content.cursor.point.column.0;
            let cell = &term.grid()[content.cursor.point];
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            screen.cursor = Some(Cursor {
                line,
                column,
                shape: content.cursor.shape,
                color: palette.resolve(Color::Named(NamedColor::Cursor), overrides),
                wide,
                text: (!is_blank(cell)).then(|| TextSpan {
                    line,
                    column,
                    text: cell_text(cell),
                    width: if wide { 2 } else { 1 },
                    wide,
                    style: style(cell, background),
                }),
            });
        }
        screen
    }

    fn push_fill(&mut self, line: usize, column: usize, width: usize, color: Rgb) {
        if let Some(last) = self.fills.last_mut()
            && last.line == line
            && last.column + last.width == column
            && last.color == color
        {
            last.width += width;
            return;
        }
        self.fills.push(Fill {
            line,
            column,
            width,
            color,
        });
    }

    fn push_text(&mut self, line: usize, column: usize, cell: &Cell, wide: bool, style: TextStyle) {
        if !wide
            && let Some(last) = self.spans.last_mut()
            && !last.wide
            && last.line == line
            && last.column + last.width == column
            && last.style == style
        {
            last.text.push_str(&cell_text(cell));
            last.width += 1;
            return;
        }
        self.spans.push(TextSpan {
            line,
            column,
            text: cell_text(cell),
            width: if wide { 2 } else { 1 },
            wide,
            style,
        });
    }
}

fn style(cell: &Cell, color: Rgb) -> TextStyle {
    TextStyle {
        color,
        bold: cell.flags.contains(Flags::BOLD),
        italic: cell.flags.contains(Flags::ITALIC),
        underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
        strikeout: cell.flags.contains(Flags::STRIKEOUT),
    }
}

/// A cell with nothing to draw but its background.
fn is_blank(cell: &Cell) -> bool {
    (cell.c == ' ' || cell.c == '\t')
        && cell.zerowidth().is_none()
        && !cell
            .flags
            .intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
}

fn cell_text(cell: &Cell) -> String {
    let mut text = String::from(cell.c);
    text.extend(cell.zerowidth().into_iter().flatten());
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridSize;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Scroll;
    use alacritty_terminal::index::{Column, Line, Point, Side};
    use alacritty_terminal::selection::{Selection, SelectionType};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(output: &str) -> Term<VoidListener> {
        let size = GridSize {
            columns: 20,
            lines: 4,
            cell_width: 8,
            cell_height: 16,
        };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, output.as_bytes());
        term
    }

    fn texts(screen: &Screen) -> Vec<(usize, usize, &str)> {
        screen
            .spans
            .iter()
            .map(|span| (span.line, span.column, span.text.as_str()))
            .collect()
    }

    #[test]
    fn spans_split_on_blanks_and_style_changes() {
        let screen = Screen::capture(&term("ab cd\x1b[1mef\r\nx"), &Palette::default());
        assert_eq!(
            texts(&screen),
            [(0, 0, "ab"), (0, 3, "cd"), (0, 5, "ef"), (1, 0, "x")]
        );
        assert!(screen.spans[2].style.bold);
        assert!(screen.fills.is_empty());
    }

    #[test]
    fn wide_characters_get_their_own_span() {
        let screen = Screen::capture(&term("a中b"), &Palette::default());
        assert_eq!(texts(&screen), [(0, 0, "a"), (0, 1, "中"), (0, 3, "b")]);
        assert!(screen.spans[1].wide);
    }

    #[test]
    fn colored_backgrounds_merge_into_fills() {
        let palette = Palette::default();
        let screen = Screen::capture(&term("\x1b[41m  \x1b[0m "), &palette);
        assert_eq!(
            screen.fills,
            [Fill {
                line: 0,
                column: 0,
                width: 2,
                color: palette.ansi[1],
            }]
        );
    }

    #[test]
    fn truecolor_tool_output_follows_the_light_terminal_theme() {
        let output = "\x1b[48;2;32;54;42m\x1b[38;2;243;243;243mHi\x1b[0m";
        let palette = Palette::light();
        let screen = Screen::capture(&term(output), &palette);
        assert_eq!(screen.spans[0].style.color, palette.foreground);
        assert!(screen.fills[0].color.r > 200);
        assert_ne!(screen.fills[0].color, palette.background);

        let dark = Screen::capture(&term(output), &Palette::default());
        assert_eq!(
            dark.fills[0].color,
            Rgb {
                r: 32,
                g: 54,
                b: 42
            }
        );
    }

    #[test]
    fn selection_is_filled() {
        let palette = Palette::default();
        let mut term = term("hello");
        let mut selection = Selection::new(
            SelectionType::Simple,
            Point::new(Line(0), Column(1)),
            Side::Left,
        );
        selection.update(Point::new(Line(0), Column(3)), Side::Right);
        term.selection = Some(selection);
        let screen = Screen::capture(&term, &palette);
        assert_eq!(screen.fills.len(), 1);
        assert_eq!((screen.fills[0].column, screen.fills[0].width), (1, 3));
        assert_eq!(screen.fills[0].color, palette.selection);
    }

    #[test]
    fn hidden_cursor_keeps_its_position() {
        let screen = Screen::capture(&term("ab\x1b[?25l"), &Palette::default());
        let cursor = screen.cursor.expect("cursor on screen");
        assert_eq!((cursor.line, cursor.column), (0, 2));
        assert_eq!(cursor.shape, CursorShape::Hidden);
    }

    #[test]
    fn cursor_carries_the_character_beneath_it() {
        let screen = Screen::capture(&term("abc\x1b[2D"), &Palette::default());
        let cursor = screen.cursor.expect("cursor on screen");
        assert_eq!(cursor.text.expect("character under cursor").text, "b");
    }

    #[test]
    fn scrolled_back_view_hides_the_cursor() {
        let mut term = term(&"line\r\n".repeat(10));
        term.scroll_display(Scroll::Delta(5));
        let screen = Screen::capture(&term, &Palette::default());
        assert_eq!(screen.cursor, None);
        assert_eq!(screen.spans.len(), 4);
    }
}
