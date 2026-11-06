//! Marks beside the editor's line numbers for lines that differ from the
//! last commit, kept in step with unsaved edits.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use koi_core::line_changes::{LineChange, LineChangeKind, line_changes};

/// How long typing has to pause before the lines are compared again.
const EDIT_DELAY: Duration = Duration::from_millis(300);
const MARK_WIDTH: Pixels = px(3.);

/// The changed lines of one editor.
pub struct ChangeGutter {
    editor: WeakEntity<EditorState>,
    /// The committed text, or `None` when the file has nothing to compare with.
    base: Option<Arc<str>>,
    changes: Arc<[LineChange]>,
    pending: Option<Task<()>>,
}

impl ChangeGutter {
    pub fn new(editor: &Entity<EditorState>) -> Self {
        Self {
            editor: editor.downgrade(),
            base: None,
            changes: Arc::new([]),
            pending: None,
        }
    }

    /// Compares the editor with `base` from now on, starting at once.
    pub fn set_base(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        self.base = base.map(Arc::from);
        self.compare(Duration::ZERO, cx);
    }

    /// Compares again once typing pauses.
    pub fn edited(&mut self, cx: &mut Context<Self>) {
        self.compare(EDIT_DELAY, cx);
    }

    fn compare(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(base) = self.base.clone() else {
            self.pending = None;
            self.changes = Arc::new([]);
            cx.notify();
            return;
        };
        let editor = self.editor.clone();
        self.pending = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(text) = editor.read_with(cx, |editor, _| editor.value()) else {
                return;
            };
            let changes = cx
                .background_spawn(async move { line_changes(&base, &text) })
                .await;
            this.update(cx, |this, cx| {
                this.changes = changes.into();
                cx.notify();
            })
            .ok();
        }));
    }

    /// An overlay for the editor that paints the marks at the left edge of
    /// its line numbers. It is drawn after the editor, so the editor's layout
    /// of this frame is used.
    pub fn overlay(&self, editor: &Entity<EditorState>, cx: &App) -> impl IntoElement {
        let changes = self.changes.clone();
        let editor = editor.clone();
        let theme = cx.theme();
        let colors = [theme.success, theme.warning, theme.danger];
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
                    for change in changes.iter() {
                        let top = text.origin.y + line_height * change.lines.start as f32;
                        let (color, top, height) = match change.kind {
                            LineChangeKind::Added => {
                                (colors[0], top, line_height * change.lines.len() as f32)
                            }
                            LineChangeKind::Modified => {
                                (colors[1], top, line_height * change.lines.len() as f32)
                            }
                            LineChangeKind::Deleted => {
                                let height = line_height * 0.4;
                                (colors[2], top - height / 2., height)
                            }
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
}
