//! The overview: one card per session with its state, place, resource usage and
//! latest output. Clicking a card opens the session.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::*;
use koi_core::SessionRecord;
use rust_i18n::t;

use super::{WorkspaceView, directory_name};
use crate::settings::code_font;

const CARD_WIDTH: Pixels = px(360.);

impl WorkspaceView {
    pub(super) fn render_overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.workspace.sessions.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(gpui_kit::assets::IconName::Bot).size_8())
                .child(t!("overview.empty"))
                .into_any_element();
        }
        let cards: Vec<AnyElement> = self
            .workspace
            .sessions
            .iter()
            .enumerate()
            .map(|(ix, record)| self.render_card(ix, record, cx))
            .collect();
        div()
            .id("overview")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .child(h_flex().flex_wrap().items_start().gap_3().children(cards))
            .into_any_element()
    }

    fn render_card(&self, ix: usize, record: &SessionRecord, cx: &mut Context<Self>) -> AnyElement {
        let id = record.id;
        let status = self.status(id, cx);
        let running = self.running.get(&id);
        let recent = running.and_then(|running| running.view.read(cx).recent_line());
        let project = directory_name(&record.project);
        let theme = cx.theme();
        v_flex()
            .id(("card", ix))
            .w(CARD_WIDTH)
            .max_w_full()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .cursor_pointer()
            .hover(|card| card.border_color(theme.ring))
            .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .child(status.dot(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_semibold()
                            .child(self.session_name(record)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(status.label()),
                    )
                    .child(
                        Button::new(("close-card", ix))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("action.close_session"))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.confirm_close_session(id, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        match self.change_count(&record.dir).filter(|&count| count > 0) {
                            Some(count) => format!(
                                "{project} · {} · {}",
                                self.place_name(record),
                                t!("workspace.change_count", count = count)
                            ),
                            None => format!("{project} · {}", self.place_name(record)),
                        },
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .font_family(code_font(cx).family)
                    .text_color(theme.muted_foreground)
                    .child(recent.unwrap_or_else(|| "—".to_owned())),
            )
            .child(self.render_session_metrics(id, cx))
            .into_any_element()
    }
}
