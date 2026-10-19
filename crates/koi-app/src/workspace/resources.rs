//! Memory and CPU of each session and of Koi itself, sampled while the
//! window is active and shown in the status bar, the list and the overview.
//! The latest replies of agent sessions are refreshed on the same beat.

use std::time::Duration;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex};
use gpui_kit::*;
use koi_core::resources::{ProcessTable, Usage, format_memory};
use koi_core::{SessionId, reentry_change};
use rust_i18n::t;

use super::WorkspaceView;
use crate::app_menu::OpenSettings;

/// How often processes are sampled while the window is active.
const INTERVAL: Duration = Duration::from_secs(3);
const STATUS_BAR_HEIGHT: Pixels = px(24.);

impl WorkspaceView {
    /// Samples resources while the window is active and stops otherwise, so
    /// an app in the background does not wake up for numbers nobody sees.
    pub(super) fn track_resources(
        &mut self,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !active {
            self.sampler = None;
            return;
        }
        if self.sampler.is_some() {
            return;
        }
        self.sampler = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let table = cx
                    .background_executor()
                    .spawn(async { ProcessTable::read() })
                    .await;
                let Ok(replies) = this.update_in(cx, |this, window, cx| {
                    this.apply_sample(table.ok(), window, cx);
                    this.refresh_replies(cx)
                }) else {
                    break;
                };
                replies.await;
                cx.background_executor().timer(INTERVAL).await;
            }
        }));
    }

    /// Records a sample; an unreadable process list clears the numbers
    /// rather than showing stale ones.
    fn apply_sample(
        &mut self,
        table: Option<ProcessTable>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.usage.clear();
        self.app_memory = None;
        if let Some(table) = table {
            // Agents started or left by hand in a session's shell.
            let mut changes: Vec<(SessionId, Option<String>)> = Vec::new();
            for (id, running) in &self.running {
                let terminal = running.view.read(cx);
                if terminal.exit_code().is_some() {
                    continue;
                }
                let Some(pid) = terminal.pid() else {
                    continue;
                };
                if let Some(usage) = table.tree(pid) {
                    self.usage.insert(*id, usage);
                }
                let below = table.agent_below(pid, &self.agents);
                if let Some(found) = reentry_change(
                    running.shell,
                    running.reentered.as_deref(),
                    below.map(|agent| agent.id.as_str()),
                ) {
                    changes.push((*id, found.map(str::to_owned)));
                }
            }
            for (id, found) in changes {
                match found {
                    Some(agent) => self.enter_agent(id, &agent, window, cx),
                    None => self.return_to_shell(id, window, cx),
                }
            }
            self.app_memory = table.process(std::process::id()).map(|usage| usage.memory);
        }
        cx.notify();
    }

    /// Compact resource values shared by the sidebar and the overview.
    pub(super) fn render_session_metrics(&self, id: SessionId, cx: &App) -> impl IntoElement {
        let usage = self.usage.get(&id);
        let cpu = usage.map_or_else(|| "—".to_owned(), |usage| format!("{:.1}%", usage.cpu));
        let memory = usage.map_or_else(|| "—".to_owned(), |usage| format_memory(usage.memory));
        h_flex()
            .flex_none()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(format!("{} {cpu}", t!("resources.cpu")))
            .child("·")
            .child(format!("{} {memory}", t!("resources.rss")))
    }

    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let agents = self
            .usage
            .values()
            .fold(Usage::default(), |total, usage| total + *usage);
        let running = self
            .running
            .values()
            .filter(|running| running.view.read(cx).exit_code().is_none())
            .count();
        let theme = cx.theme();
        h_flex()
            .h(STATUS_BAR_HEIGHT)
            .flex_none()
            .px_3()
            .gap_4()
            .text_xs()
            .text_color(theme.muted_foreground)
            .bg(theme.status_bar)
            .border_t_1()
            .border_color(theme.status_bar_border)
            .children(
                self.app_memory
                    .map(|memory| t!("resources.app", memory = format_memory(memory))),
            )
            .children((!self.usage.is_empty()).then(|| {
                t!(
                    "resources.agents",
                    memory = format_memory(agents.memory),
                    cpu = format!("{:.0}", agents.cpu)
                )
            }))
            .child(div().flex_1())
            .child(t!("resources.running", count = running))
            .child(
                h_flex()
                    .id("status-settings")
                    .test_support()
                    .w(px(20.))
                    .h(px(20.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .role(gpui_kit::accesskit::Role::Button)
                    .tab_stop(true)
                    .hover(|button| button.bg(theme.foreground.opacity(0.12)))
                    .tooltip(|window, cx| {
                        Tooltip::new(t!("action.settings").into_owned()).build(window, cx)
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(OpenSettings), cx);
                    })
                    .on_key_down(|event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            window.dispatch_action(Box::new(OpenSettings), cx);
                            cx.stop_propagation();
                        }
                    })
                    .child(
                        Icon::new(gpui_kit::assets::IconName::Settings)
                            .small()
                            .text_color(theme.muted_foreground),
                    ),
            )
    }
}
