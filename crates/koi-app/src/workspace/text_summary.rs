//! What the editor's side panels show about its text: the lines that differ
//! from the last commit and the shape of every line, kept in step with
//! unsaved edits.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use koi_core::line_changes::{LineChange, line_changes};
use koi_core::minimap::{Run, line_runs};

/// How long typing has to pause before the text is looked at again.
const EDIT_DELAY: Duration = Duration::from_millis(300);

/// The analyzed text of one editor.
pub struct TextSummary {
    editor: WeakEntity<EditorState>,
    /// The committed text, or `None` when the file has nothing to compare with.
    base: Option<Arc<str>>,
    pub changes: Arc<[LineChange]>,
    /// Non-blank runs of each line.
    pub shape: Arc<[Box<[Run]>]>,
    pending: Option<Task<()>>,
}

impl TextSummary {
    pub fn new(editor: &Entity<EditorState>) -> Self {
        Self {
            editor: editor.downgrade(),
            base: None,
            changes: Arc::new([]),
            shape: Arc::new([]),
            pending: None,
        }
    }

    /// Compares the editor with `base` from now on, starting at once.
    pub fn set_base(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        self.base = base.map(Arc::from);
        self.analyze(Duration::ZERO, cx);
    }

    /// Looks at the text again once typing pauses.
    pub fn edited(&mut self, cx: &mut Context<Self>) {
        self.analyze(EDIT_DELAY, cx);
    }

    fn analyze(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let (editor, base) = (self.editor.clone(), self.base.clone());
        self.pending = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(text) = editor.read_with(cx, |editor, _| editor.value()) else {
                return;
            };
            let (changes, shape) = cx
                .background_spawn(async move {
                    let changes = base.map_or_else(Vec::new, |base| line_changes(&base, &text));
                    (changes, line_runs(&text))
                })
                .await;
            this.update(cx, |this, cx| {
                this.changes = changes.into();
                this.shape = shape.into();
                cx.notify();
            })
            .ok();
        }));
    }
}
