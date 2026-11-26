//! Memory and CPU of each session and of Koi itself, sampled while the
//! window is active or the services window is open, and shown in the status
//! bar, the list and the overview. Background services and the latest
//! replies of agent sessions are refreshed on the same beat.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex};
use gpui_kit::*;
use koi_core::resources::{ProcessTable, Usage, format_memory};
use koi_core::services::{self, Ports};
use koi_core::{SessionId, reentry_change};
use rust_i18n::t;

use super::WorkspaceView;
use crate::app_menu::{OpenServices, OpenSettings};

/// How often processes are sampled while the window is active.
pub const INTERVAL: Duration = Duration::from_secs(3);
const STATUS_BAR_HEIGHT: Pixels = px(24.);

impl WorkspaceView {
    /// Samples resources while the main window is active and stops
    /// otherwise, so an app in the background does not wake up for numbers
    /// nobody sees. The services window samples on its own while open.
    pub(super) fn track_resources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            self.sampler = None;
            return;
        }
        if self.sampler.is_some() {
            return;
        }
        self.sampler = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let Ok(sampled) = this.update_in(cx, |this, window, cx| this.sample(window, cx))
                else {
                    break;
                };
                sampled.await;
                cx.background_executor().timer(INTERVAL).await;
            }
        }));
    }

    /// Takes one sample in the background and records it.
    pub(super) fn sample(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        let read =
            cx.background_spawn(async { (ProcessTable::read(), services::listening_ports()) });
        cx.spawn_in(window, async move |this, cx| {
            let (table, ports) = read.await;
            let Ok(replies) = this.update_in(cx, |this, window, cx| {
                // Without `lsof`, services are still found by leaving their terminal.
                this.apply_sample(table.ok(), ports.unwrap_or_default(), window, cx);
                this.refresh_replies(cx)
            }) else {
                return;
            };
            replies.await;
        })
    }

    /// Records a sample; an unreadable process list clears the numbers
    /// rather than showing stale ones.
    fn apply_sample(
        &mut self,
        table: Option<ProcessTable>,
        ports: Ports,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.usage.clear();
        self.app_memory = None;
        self.services.clear();
        if let Some(table) = table {
            self.services = services::find(
                &table,
                &ports,
                &self.owners(cx),
                &self.agents,
                std::process::id(),
            );
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
        // Sessions of hidden projects keep running but are not counted.
        let running = self
            .workspace
            .shown_sessions()
            .filter_map(|session| self.running.get(&session.id))
            .filter(|running| running.view.read(cx).exit_code().is_none())
            .count();
        let services = self.service_count();
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
            .child(status_button::<OpenServices>(
                "status-services",
                IconName::Server,
                t!("action.services").into(),
                (services > 0).then(|| services.to_string()),
                cx,
            ))
            .child(status_button::<OpenSettings>(
                "status-settings",
                IconName::Settings,
                t!("action.settings").into(),
                None,
                cx,
            ))
    }
}

/// An icon button in the status bar dispatching `A`, with `badge` beside
/// the icon when given.
fn status_button<A: Action + Default>(
    id: &'static str,
    icon: IconName,
    tooltip: SharedString,
    badge: Option<String>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .test_support()
        .min_w(px(20.))
        .h(px(20.))
        .px_0p5()
        .gap_1()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .role(gpui_kit::accesskit::Role::Button)
        .tab_stop(true)
        .hover(|button| button.bg(theme.foreground.opacity(0.12)))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(A::default()), cx))
        .on_key_down(|event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                window.dispatch_action(Box::new(A::default()), cx);
                cx.stop_propagation();
            }
        })
        .child(Icon::new(icon).small().text_color(theme.muted_foreground))
        .children(badge)
}
