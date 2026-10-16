//! Lays out the terminal grid and paints a captured [`Screen`].

use gpui_kit::*;
use koi_term::state::vte::ansi::{CursorShape, Rgb};
use koi_term::{Cursor, GridSize, Screen, TextSpan};

use super::TerminalView;
use crate::settings::{CodeFont, code_font};

const LINE_HEIGHT_SCALE: f32 = 1.3;
const PADDING: Pixels = px(8.);
const CURSOR_BAR: Pixels = px(2.);
const DECORATION: Pixels = px(1.);

/// Where the grid sits in window coordinates.
#[derive(Clone, Copy)]
pub struct Geometry {
    pub origin: Point<Pixels>,
    pub cell: Size<Pixels>,
}

impl Geometry {
    /// Bounds of `width` cells starting at a viewport cell.
    pub fn cells(&self, line: usize, column: usize, width: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.cell.width * column as f32,
                self.origin.y + self.cell.height * line as f32,
            ),
            size(self.cell.width * width as f32, self.cell.height),
        )
    }
}

/// Everything painted in one frame, captured during prepaint.
pub struct Frame {
    screen: Screen,
    geometry: Geometry,
    font: CodeFont,
    focused: bool,
    marked_text: Option<String>,
}

/// The element that draws `view`'s terminal and receives its text input.
pub fn grid(view: Entity<TerminalView>) -> Canvas<Frame> {
    let input_view = view.clone();
    canvas(
        move |bounds, window, cx| {
            view.update(cx, |view, cx| view.prepare_frame(bounds, window, cx))
        },
        move |bounds, frame, window, cx| {
            paint_frame(bounds, &frame, window, cx);
            let focus = input_view.read(cx).focus.clone();
            window.handle_input(&focus, ElementInputHandler::new(bounds, input_view), cx);
        },
    )
}

impl TerminalView {
    /// Fits the grid to `bounds` and captures the screen to paint.
    fn prepare_frame(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &App) -> Frame {
        let code_font = code_font(cx);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(code_font.family.clone()));
        let cell_width = text_system
            .advance(font_id, code_font.size, 'm')
            .map_or(code_font.size * 0.6, |advance| advance.width);
        let cell = size(cell_width, (code_font.size * LINE_HEIGHT_SCALE).round());
        let fit =
            |available: Pixels, step: Pixels| ((available - PADDING * 2.) / step).floor().max(1.);
        self.session.resize(GridSize {
            columns: fit(bounds.size.width, cell.width) as u16,
            lines: fit(bounds.size.height, cell.height) as u16,
            cell_width: f32::from(cell.width) as u16,
            cell_height: f32::from(cell.height) as u16,
        });

        let geometry = Geometry {
            origin: bounds.origin + point(PADDING, PADDING),
            cell,
        };
        self.geometry = Some(geometry);
        Frame {
            screen: Screen::capture(&self.session.term(), &self.palette),
            geometry,
            font: code_font,
            focused: self.focus.is_focused(window),
            marked_text: self.marked_text.clone(),
        }
    }
}

fn paint_frame(bounds: Bounds<Pixels>, frame: &Frame, window: &mut Window, cx: &mut App) {
    let screen = &frame.screen;
    window.paint_quad(fill(bounds, to_hsla(screen.background)));
    for area in &screen.fills {
        let cells = frame.geometry.cells(area.line, area.column, area.width);
        window.paint_quad(fill(cells, to_hsla(area.color)));
    }
    for span in &screen.spans {
        paint_text(span, span.style.color, frame, window, cx);
    }
    if let Some(cursor) = &screen.cursor {
        match &frame.marked_text {
            Some(text) => paint_marked_text(text, cursor, frame, window, cx),
            None => paint_cursor(cursor, frame, window, cx),
        }
    }
}

fn paint_text(span: &TextSpan, color: Rgb, frame: &Frame, window: &mut Window, cx: &mut App) {
    let geometry = &frame.geometry;
    let color = to_hsla(color);
    let style = span.style;
    let run = TextRun {
        len: span.text.len(),
        font: Font {
            weight: if style.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if style.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..font(frame.font.family.clone())
        },
        color,
        background_color: None,
        underline: style.underline.then_some(UnderlineStyle {
            thickness: DECORATION,
            color: Some(color),
            wavy: false,
        }),
        strikethrough: style.strikeout.then_some(StrikethroughStyle {
            thickness: DECORATION,
            color: Some(color),
        }),
    };
    // Force every glyph onto the grid, including fallback-font glyphs.
    let glyph_width = geometry.cell.width * if span.wide { 2. } else { 1. };
    let line = window.text_system().shape_line(
        span.text.clone().into(),
        frame.font.size,
        &[run],
        Some(glyph_width),
    );
    let origin = geometry.cells(span.line, span.column, span.width).origin;
    // Painting fails only if a glyph cannot be rasterized; the cell stays blank.
    line.paint(
        origin,
        geometry.cell.height,
        TextAlign::Left,
        None,
        window,
        cx,
    )
    .ok();
}

fn paint_cursor(cursor: &Cursor, frame: &Frame, window: &mut Window, cx: &mut App) {
    let cell = frame
        .geometry
        .cells(cursor.line, cursor.column, if cursor.wide { 2 } else { 1 });
    let color = to_hsla(cursor.color);
    match (frame.focused, cursor.shape) {
        (_, CursorShape::Hidden) => {}
        (false, _) | (true, CursorShape::HollowBlock) => {
            window.paint_quad(outline(cell, color, BorderStyle::Solid));
        }
        (true, CursorShape::Block) => {
            window.paint_quad(fill(cell, color));
            if let Some(text) = &cursor.text {
                paint_text(text, text.style.color, frame, window, cx);
            }
        }
        (true, CursorShape::Beam) => {
            window.paint_quad(fill(
                Bounds::new(cell.origin, size(CURSOR_BAR, cell.size.height)),
                color,
            ));
        }
        (true, CursorShape::Underline) => {
            let origin = point(cell.origin.x, cell.bottom() - CURSOR_BAR);
            window.paint_quad(fill(
                Bounds::new(origin, size(cell.size.width, CURSOR_BAR)),
                color,
            ));
        }
    }
}

/// Shows text an input method is still composing, over the cursor position.
fn paint_marked_text(
    text: &str,
    cursor: &Cursor,
    frame: &Frame,
    window: &mut Window,
    cx: &mut App,
) {
    let (screen, geometry) = (&frame.screen, &frame.geometry);
    let color = to_hsla(screen.foreground);
    let run = TextRun {
        len: text.len(),
        font: font(frame.font.family.clone()),
        color,
        background_color: None,
        underline: Some(UnderlineStyle {
            thickness: DECORATION,
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    };
    let line =
        window
            .text_system()
            .shape_line(text.to_owned().into(), frame.font.size, &[run], None);
    let origin = geometry.cells(cursor.line, cursor.column, 1).origin;
    let area = Bounds::new(origin, size(line.width, geometry.cell.height));
    window.paint_quad(fill(area, to_hsla(screen.background)));
    line.paint(
        origin,
        geometry.cell.height,
        TextAlign::Left,
        None,
        window,
        cx,
    )
    .ok();
}

pub fn to_hsla(color: Rgb) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: 1.,
    }
    .into()
}
