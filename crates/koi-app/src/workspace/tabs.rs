//! The tab bar above the main area: the overview, then opened sessions.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use koi_core::{SessionRecord, SplitSide};
use rust_i18n::t;

use super::views::Panel;
use super::{ShowOverview, WorkspaceView};

const HEIGHT: Pixels = px(36.);
/// Widest a session name gets in its tab; longer names end in an ellipsis.
const TAB_NAME_WIDTH: Pixels = px(200.);

/// A tab: highlighted when active, with hover feedback otherwise.
fn tab(id: impl Into<ElementId>, active: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .h_full()
        .flex_none()
        .px_3()
        .gap_2()
        .text_sm()
        .border_r_1()
        .border_color(theme.border)
        .cursor_pointer()
        .when(active, |tab| {
            tab.bg(theme.tab_active)
                .text_color(theme.tab_active_foreground)
        })
        .when(!active, |tab| {
            tab.text_color(theme.tab_foreground)
                .hover(|tab| tab.bg(theme.tab_active.opacity(0.5)))
        })
}

impl WorkspaceView {
    pub(super) fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let overview = tab("overview-tab", self.workspace.active.is_none(), cx)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ShowOverview), cx))
            .child(t!("tabs.overview"));
        let sessions: Vec<AnyElement> = self
            .workspace
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(ix, &id)| Some(self.render_tab(ix, self.workspace.session(id)?, cx)))
            .collect();
        let switch = self.render_view_switch(cx);
        let theme = cx.theme();
        h_flex()
            .h(HEIGHT)
            .flex_none()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .id("tab-bar")
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .child(overview)
                    .children(sessions),
            )
            .children(switch)
    }

    /// Buttons switching the active session between its terminal and the
    /// changes or files of its worktree; none on the overview.
    fn render_view_switch(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let record = self.workspace.session(self.workspace.active?)?;
        let current = self.active_panel();
        let changes_label = match self.change_count(&record.dir) {
            Some(count) if count > 0 => t!("tabs.changes_count", count = count),
            _ => t!("tabs.changes"),
        };
        let buttons = [
            (
                "show-terminal",
                None,
                gpui_kit::assets::IconName::Terminal,
                t!("tabs.terminal"),
            ),
            (
                "show-changes",
                Some(Panel::Changes),
                gpui_kit::assets::IconName::GitCompare,
                changes_label,
            ),
            (
                "show-files",
                Some(Panel::Files),
                gpui_kit::assets::IconName::Files,
                t!("tabs.files"),
            ),
        ];
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_none()
                .px_2()
                .gap_1()
                .children(buttons.into_iter().map(|(id, panel, icon, label)| {
                    let selected = current == panel;
                    let tooltip = label.into_owned();
                    let selected_bg = theme.foreground.opacity(0.20);
                    let hover_bg = if selected {
                        theme.foreground.opacity(0.28)
                    } else {
                        theme.foreground.opacity(0.10)
                    };
                    h_flex()
                        .id(id)
                        .test_support()
                        .w(px(28.))
                        .h(px(28.))
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .cursor_pointer()
                        .role(gpui_kit::accesskit::Role::Button)
                        .tab_stop(true)
                        .when(selected, |button| {
                            button
                                .bg(selected_bg)
                                .border_b_1()
                                .border_color(theme.foreground.opacity(0.45))
                        })
                        .hover(|button| button.bg(hover_bg))
                        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_panel(panel, window, cx);
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.show_panel(panel, window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(Icon::new(icon).small().text_color(if selected {
                            theme.foreground
                        } else {
                            theme.tab_foreground
                        }))
                }))
                .into_any_element(),
        )
    }

    fn render_tab(&self, ix: usize, record: &SessionRecord, cx: &mut Context<Self>) -> AnyElement {
        let id = record.id;
        let active = self.workspace.active == Some(id);
        tab(("session-tab", ix), active, cx)
            .pr_1()
            .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
            .child(self.status(id, cx).dot(cx))
            .child(
                div()
                    .max_w(TAB_NAME_WIDTH)
                    .truncate()
                    .child(self.session_name(record)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.place_name(record)),
            )
            .child(
                Button::new(("close-tab", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip(t!("action.close_tab"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_tab(id, window, cx);
                    })),
            )
            .context_menu({
                let workspace = cx.entity().downgrade();
                move |menu, _, _| {
                    // The active tab is already on screen and cannot also be split.
                    let menu = [
                        (SplitSide::Right, t!("split.right")),
                        (SplitSide::Below, t!("split.below")),
                    ]
                    .into_iter()
                    .fold(menu, |menu, (side, label)| {
                        let workspace = workspace.clone();
                        menu.item(PopupMenuItem::new(label).disabled(active).on_click(
                            move |_, window, cx| {
                                workspace
                                    .update(cx, |this, cx| this.split_session(id, side, window, cx))
                                    .ok();
                            },
                        ))
                    });
                    let workspace = workspace.clone();
                    menu.separator()
                        .item(PopupMenuItem::new(t!("detached.open")).on_click(
                            move |_, window, cx| {
                                workspace
                                    .update(cx, |this, cx| this.detach(id, window, cx))
                                    .ok();
                            },
                        ))
                }
            })
            .into_any_element()
    }
}
