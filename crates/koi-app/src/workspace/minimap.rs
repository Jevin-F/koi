//! A narrow picture of the whole file beside the editor, showing where the
//! editor is and letting a click or drag move it there.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use koi_core::line_changes::LineChangeKind;
use koi_core::minimap::MinimapGeometry;

use super::change_gutter::change_color;
use super::text_summary::TextSummary;

pub const MINIMAP_WIDTH: Pixels = px(90.);
const LINE_HEIGHT: Pixels = px(2.);
const COLUMN_WIDTH: Pixels = px(1.);
/// Room on the left for the change marks.
const TEXT_LEFT: Pixels = px(6.);
const MARK_WIDTH: Pixels = px(2.);

/// The minimap of one editor.
#[derive(Default)]
pub struct Minimap {
    /// Where the minimap was last painted, for turning clicks into lines.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    dragging: Rc<Cell<bool>>,
}

/// The editor's scroll state as the minimap needs it.
struct View {
    geometry: MinimapGeometry,
    line_height: Pixels,
    scroll: Point<Pixels>,
}

fn view(lines: usize, editor: &EditorState, bounds: Bounds<Pixels>) -> Option<View> {
    let line_height = editor.line_height()?;
    let scroll = editor.scroll_offset();
    let geometry = MinimapGeometry {
        lines: lines as f32,
        visible: editor.input_bounds().size.height / line_height,
        top: -scroll.y / line_height,
        capacity: bounds.size.height / LINE_HEIGHT,
    };
    Some(View {
        geometry,
        line_height,
        scroll,
    })
}

/// Scrolls the editor so the line under `position` is in its middle.
fn scroll_to(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    summary: &Entity<TextSummary>,
    editor: &Entity<EditorState>,
    cx: &mut App,
) {
    let lines = summary.read(cx).shape.len();
    let Some(view) = view(lines, editor.read(cx), bounds) else {
        return;
    };
    let line = view.geometry.first_line() + (position.y - bounds.top()) / LINE_HEIGHT;
    let top = view.geometry.top_centering(line);
    let offset = point(view.scroll.x, -(view.line_height * top));
    editor.update(cx, |editor, cx| editor.set_scroll_offset(offset, cx));
}

impl Minimap {
    pub fn render(
        &self,
        summary: &Entity<TextSummary>,
        editor: &Entity<EditorState>,
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let ink = theme.foreground.opacity(0.35);
        let window_color = theme.foreground.opacity(0.08);
        let (painted, paint_summary, paint_editor) =
            (self.bounds.clone(), summary.clone(), editor.clone());
        let picture = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                painted.set(bounds);
                let summary = paint_summary.read(cx);
                let Some(view) = view(summary.shape.len(), paint_editor.read(cx), bounds) else {
                    return;
                };
                let first = view.geometry.first_line();
                let row_top = |line: f32| bounds.top() + LINE_HEIGHT * (line - first);
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let shown = first as usize..(first + view.geometry.capacity) as usize + 1;
                    for (line, runs) in summary
                        .shape
                        .iter()
                        .enumerate()
                        .take(shown.end)
                        .skip(shown.start)
                    {
                        let top = row_top(line as f32);
                        for run in runs.iter() {
                            let left = bounds.left() + TEXT_LEFT + COLUMN_WIDTH * run.start as f32;
                            let width = COLUMN_WIDTH * run.len as f32;
                            window.paint_quad(fill(
                                Bounds::new(point(left, top), size(width, LINE_HEIGHT)),
                                ink,
                            ));
                        }
                    }
                    let theme = cx.theme();
                    for change in summary.changes.iter() {
                        if change.lines.end < shown.start || change.lines.start > shown.end {
                            continue;
                        }
                        let lines = change.lines.len().max(1) as f32;
                        let top = match change.kind {
                            LineChangeKind::Deleted => {
                                row_top(change.lines.start as f32) - LINE_HEIGHT / 2.
                            }
                            _ => row_top(change.lines.start as f32),
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                point(bounds.left(), top),
                                size(MARK_WIDTH, LINE_HEIGHT * lines),
                            ),
                            change_color(change.kind, theme),
                        ));
                    }
                    let shown_window = Bounds::new(
                        point(bounds.left(), row_top(view.geometry.top)),
                        size(bounds.size.width, LINE_HEIGHT * view.geometry.visible),
                    );
                    window.paint_quad(fill(shown_window, window_color));
                });
            },
        )
        .size_full();

        let (press_bounds, press_summary, press_editor, press_drag) = (
            self.bounds.clone(),
            summary.clone(),
            editor.clone(),
            self.dragging.clone(),
        );
        let (move_bounds, move_summary, move_editor, move_drag) = (
            self.bounds.clone(),
            summary.clone(),
            editor.clone(),
            self.dragging.clone(),
        );
        let (release, release_out) = (self.dragging.clone(), self.dragging.clone());
        let wheel_editor = editor.clone();
        div()
            .id("minimap")
            .h_full()
            .w(MINIMAP_WIDTH)
            .flex_none()
            .border_l_1()
            .border_color(theme.border)
            .child(picture)
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                press_drag.set(true);
                scroll_to(
                    event.position,
                    press_bounds.get(),
                    &press_summary,
                    &press_editor,
                    cx,
                );
            })
            .on_mouse_move(move |event, _, cx| {
                if move_drag.get() && event.pressed_button == Some(MouseButton::Left) {
                    scroll_to(
                        event.position,
                        move_bounds.get(),
                        &move_summary,
                        &move_editor,
                        cx,
                    );
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| release.set(false))
            .on_mouse_up_out(MouseButton::Left, move |_, _, _| release_out.set(false))
            .on_scroll_wheel(move |event, _, cx| {
                let editor = wheel_editor.read(cx);
                let Some(line_height) = editor.line_height() else {
                    return;
                };
                let offset = editor.scroll_offset() + event.delta.pixel_delta(line_height);
                wheel_editor.update(cx, |editor, cx| editor.set_scroll_offset(offset, cx));
            })
    }
}
