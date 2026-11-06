//! Marks beside the editor's line numbers for lines that differ from the
//! last commit.

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Theme;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use koi_core::line_changes::{LineChange, LineChangeKind};

const MARK_WIDTH: Pixels = px(3.);

/// The color of a changed run of lines.
pub fn change_color(kind: LineChangeKind, theme: &Theme) -> Hsla {
    match kind {
        LineChangeKind::Added => theme.success,
        LineChangeKind::Modified => theme.warning,
        LineChangeKind::Deleted => theme.danger,
    }
}

/// An overlay for the editor that paints the marks at the left edge of its
/// line numbers. It is drawn after the editor, so the editor's layout of
/// this frame is used.
pub fn change_gutter(
    changes: Arc<[LineChange]>,
    editor: &Entity<EditorState>,
    cx: &App,
) -> impl IntoElement {
    let editor = editor.clone();
    let colors: Vec<Hsla> = changes
        .iter()
        .map(|change| change_color(change.kind, cx.theme()))
        .collect();
    canvas(
        |_, _, _| {},
        move |_, _, window, cx| {
            let editor = editor.read(cx);
            let (Some(text), Some(line_height)) = (editor.text_bounds(), editor.line_height())
            else {
                return;
            };
            let viewport = editor.input_bounds();
            window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
                for (change, color) in changes.iter().zip(colors) {
                    let top = text.origin.y + line_height * change.lines.start as f32;
                    let (top, height) = match change.kind {
                        LineChangeKind::Deleted => {
                            let height = line_height * 0.4;
                            (top - height / 2., height)
                        }
                        _ => (top, line_height * change.lines.len() as f32),
                    };
                    if top + height < viewport.top() || top > viewport.bottom() {
                        continue;
                    }
                    let mark = Bounds::new(point(text.origin.x, top), size(MARK_WIDTH, height));
                    window.paint_quad(fill(mark, color));
                }
            });
        },
    )
    .absolute()
    .inset_0()
}
