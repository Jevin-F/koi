//! Moving tabs and list rows by dragging them onto one another.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

/// Widest the preview under the pointer gets.
const PREVIEW_MAX_WIDTH: Pixels = px(260.);

/// An item being dragged; the preview under the pointer shows `name`. The
/// item's type decides where it can be dropped.
#[derive(Clone)]
pub(super) struct Dragged<T> {
    pub item: T,
    pub name: SharedString,
}

impl<T: 'static> Render for Dragged<T> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .max_w(PREVIEW_MAX_WIDTH)
            .truncate()
            .px_3()
            .py_1()
            .text_sm()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.tab_active)
            .text_color(theme.tab_active_foreground)
            .shadow_md()
            .child(self.name.clone())
    }
}

/// Creates the preview once a drag starts; pass to `on_drag`.
pub(super) fn preview<T: Clone + 'static>(
    dragged: &Dragged<T>,
    _: Point<Pixels>,
    _: &mut Window,
    cx: &mut App,
) -> Entity<Dragged<T>> {
    cx.new(|_| dragged.clone())
}
