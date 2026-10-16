//! A second session shown beside the active tab, in a pane that can be
//! resized against it.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::{ActiveTheme, IconName, Sizable, h_flex, v_flex};
use gpui_kit::*;
use koi_core::{SessionId, SplitSide};
use rust_i18n::t;

use super::WorkspaceView;

impl WorkspaceView {
    /// Shows `id` beside the active tab.
    pub(super) fn split_session(
        &mut self,
        id: SessionId,
        side: SplitSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.split(id, side) {
            self.persist(window, cx);
            cx.notify();
        }
    }

    fn close_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.split = None;
        self.after_tab_change(window, cx);
    }

    /// `content` with the split session beside it, if there is one.
    pub(super) fn with_split(&self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let Some((split, record)) = self
            .workspace
            .split
            .filter(|_| self.workspace.active.is_some())
            .and_then(|split| Some((split, self.workspace.session(split.session)?)))
        else {
            return content;
        };
        let theme = cx.theme();
        let header = h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_sm()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .child(self.status(record.id, cx).dot(cx))
            .child(self.session_name(record))
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(self.place_name(record)),
            )
            .child(
                Button::new("close-split")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip(t!("split.close"))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.close_split(window, cx))),
            );
        let body = if self.detached.contains_key(&record.id) {
            Some(self.render_detached_notice(record.id, cx))
        } else {
            self.running
                .get(&record.id)
                .map(|running| running.view.clone().into_any_element())
        };
        let pane = v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().children(body));
        let group = match split.side {
            SplitSide::Right => h_resizable("split"),
            SplitSide::Below => v_resizable("split"),
        };
        group
            .child(resizable_panel().child(content))
            .child(resizable_panel().child(pane))
            .into_any_element()
    }
}
