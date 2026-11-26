//! The services window: the background services agents started, by
//! session, each with a button to stop it. Services are sampled while the
//! window is open.

use std::collections::HashSet;

use gpui_kit::base::Disableable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, StyledExt, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use koi_core::process_stop::GRACE;
use koi_core::resources::format_memory;
use koi_core::services::{self, Service};
use rust_i18n::t;

use crate::APP_NAME;
use crate::app_menu::CloseWindow;
use crate::main_window::{self, SECONDARY_CONTEXT};
use crate::settings::code_font;
use crate::workspace::{SAMPLE_INTERVAL, SourceLabel, WorkspaceView};

const SIZE: Size<Pixels> = size(px(720.), px(560.));

/// The open services window, if any.
struct ServicesWindow(AnyWindowHandle);

impl Global for ServicesWindow {}

/// Brings the services window to the front, opening it if needed.
pub fn open(cx: &mut App) {
    // Also reached from the Dock menu while another app is in front.
    cx.activate(true);
    if let Some(handle) = cx.try_global::<ServicesWindow>().map(|window| window.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let Some(workspace) = main_window::workspace(cx) else {
        return;
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(SIZE, cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(t!("services.title", app = APP_NAME).into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |_, cx| {
        cx.new(|cx| ServicesView::new(workspace, cx))
    }) {
        Ok((handle, view)) => {
            cx.set_global(ServicesWindow(handle));
            // Focused once the window is built, so that its key context
            // closes it with cmd-w.
            handle
                .update(cx, |_, window, cx| {
                    let focus = view.read(cx).focus.clone();
                    window.focus(&focus, cx);
                })
                .ok();
        }
        Err(error) => main_window::update_workspace(cx, |_, window, cx| {
            window.push_notification(Notification::error(error.to_string()), cx);
        }),
    }
}

/// A group of services as shown, with the label of its source.
struct ShownGroup {
    label: SourceLabel,
    services: Vec<Service>,
}

struct ServicesView {
    /// The services as last sampled. Copied rather than read from the
    /// workspace while rendering: an entity read by a window's render is
    /// then updated in that window, which would move the workspace's own
    /// updates here.
    groups: Vec<ShownGroup>,
    focus: FocusHandle,
    /// Services asked to stop that are still listed.
    stopping: HashSet<u32>,
    /// Keeps services sampled while the main window is not active.
    _sampling: Task<()>,
    _workspace_changes: Subscription,
}

impl ServicesView {
    fn new(workspace: Entity<WorkspaceView>, cx: &mut Context<Self>) -> Self {
        let workspace_changes = cx.observe(&workspace, |this: &mut Self, workspace, cx| {
            this.groups = shown_groups(workspace.read(cx));
            let listed: HashSet<u32> = this
                .groups
                .iter()
                .flat_map(|group| &group.services)
                .map(|service| service.pid)
                .collect();
            this.stopping.retain(|pid| listed.contains(pid));
            cx.notify();
        });
        let sampling = cx.spawn(async move |_, cx| {
            loop {
                cx.update(|cx| main_window::update_workspace(cx, WorkspaceView::sample_services));
                cx.background_executor().timer(SAMPLE_INTERVAL).await;
            }
        });
        Self {
            groups: shown_groups(workspace.read(cx)),
            focus: cx.focus_handle(),
            stopping: HashSet::new(),
            _sampling: sampling,
            _workspace_changes: workspace_changes,
        }
    }

    /// Stops `services` in the background, then samples again.
    fn stop(&mut self, services: Vec<Service>, window: &mut Window, cx: &mut Context<Self>) {
        self.stopping
            .extend(services.iter().map(|service| service.pid));
        cx.notify();
        let stopped = cx.background_spawn(async move { services::stop(&services, GRACE) });
        cx.spawn_in(window, async move |this, cx| {
            let result = stopped.await;
            this.update_in(cx, |_, window, cx| {
                if let Err(error) = result {
                    window.push_notification(
                        Notification::error(t!("services.stop_failed", error = error)),
                        cx,
                    );
                }
                main_window::update_workspace(cx, WorkspaceView::resample);
            })
            .ok();
        })
        .detach();
    }

    /// A button stopping `services`, disabled while they all are stopping.
    fn stop_button(
        &self,
        id: impl Into<ElementId>,
        label: SharedString,
        services: Vec<Service>,
        cx: &mut Context<Self>,
    ) -> Button {
        let stopping = services
            .iter()
            .all(|service| self.stopping.contains(&service.pid));
        Button::new(id)
            .xsmall()
            .label(if stopping {
                t!("services.stopping").into()
            } else {
                label
            })
            .disabled(stopping)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.stop(services.clone(), window, cx);
            }))
    }

    fn render_group(
        &self,
        ix: usize,
        group: &ShownGroup,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = &group.label;
        let header = h_flex()
            .px_3()
            .py_2()
            .gap_3()
            .bg(cx.theme().secondary)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .truncate()
                            .child(label.title.clone()),
                    )
                    .children(label.place.clone().map(|place| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(place)
                    })),
            )
            .child(
                self.stop_button(
                    ("stop-group", ix),
                    t!("services.stop_all").into(),
                    group.services.clone(),
                    cx,
                )
                .ghost(),
            );
        let rows: Vec<AnyElement> = group
            .services
            .iter()
            .map(|service| self.render_service(service, cx).into_any_element())
            .collect();
        v_flex()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(header)
            .children(rows)
    }

    fn render_service(&self, service: &Service, cx: &mut Context<Self>) -> impl IntoElement {
        let mut details = vec![format!("PID {}", service.pid)];
        if !service.ports.is_empty() {
            let ports: Vec<String> = service
                .ports
                .iter()
                .map(|port| format!(":{port}"))
                .collect();
            details.push(ports.join(" "));
        }
        details.push(format!("{} {:.1}%", t!("resources.cpu"), service.usage.cpu));
        details.push(format!(
            "{} {}",
            t!("resources.rss"),
            format_memory(service.usage.memory)
        ));
        let theme = cx.theme();
        h_flex()
            .px_3()
            .py_2()
            .gap_3()
            .border_t_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_family(code_font(cx).family)
                            .truncate()
                            .child(service.command.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(details.join(" · ")),
                    ),
            )
            .child(
                self.stop_button(
                    ("stop-service", service.pid as usize),
                    t!("services.stop").into(),
                    vec![service.clone()],
                    cx,
                )
                .outline(),
            )
    }
}

impl Render for ServicesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let all: Vec<Service> = self
            .groups
            .iter()
            .flat_map(|group| group.services.iter().cloned())
            .collect();
        let theme = cx.theme();
        let header = h_flex()
            .flex_none()
            .px_4()
            .py_3()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_semibold()
                            .child(t!("services.count", count = all.len())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t!("services.hint")),
                    ),
            )
            .children((!all.is_empty()).then(|| {
                self.stop_button("stop-all", t!("services.stop_everything").into(), all, cx)
                    .danger()
            }));
        let body = if self.groups.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(gpui_kit::assets::IconName::Server).size_8())
                .child(t!("services.empty"))
                .into_any_element()
        } else {
            let cards: Vec<AnyElement> = self
                .groups
                .iter()
                .enumerate()
                .map(|(ix, group)| self.render_group(ix, group, cx).into_any_element())
                .collect();
            div()
                .id("services")
                .size_full()
                .overflow_y_scroll()
                .p_4()
                .child(v_flex().gap_3().children(cards))
                .into_any_element()
        };
        v_flex()
            .id("services-window")
            .track_focus(&self.focus)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .size_full()
            .key_context(SECONDARY_CONTEXT)
            .bg(cx.theme().background)
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
    }
}

/// The workspace's services, labelled by their source.
fn shown_groups(workspace: &WorkspaceView) -> Vec<ShownGroup> {
    workspace
        .services()
        .iter()
        .map(|group| ShownGroup {
            label: workspace.source_label(group.source),
            services: group.services.clone(),
        })
        .collect()
}
