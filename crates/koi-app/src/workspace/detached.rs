//! Sessions moved to windows of their own, to watch them beside the main
//! window or on another screen. Closing such a window brings the session back.

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;
use koi_core::SessionId;
use rust_i18n::t;

use super::WorkspaceView;
use crate::main_window::SECONDARY_CONTEXT;

const SIZE: Size<Pixels> = size(px(960.), px(640.));

/// A window showing one session's terminal.
struct SessionWindow {
    workspace: WeakEntity<WorkspaceView>,
    id: SessionId,
    _workspace_changes: Subscription,
    _activation: Subscription,
}

impl Render for SessionWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Read on every render, so a restarted session shows its new terminal.
        let terminal = self.workspace.upgrade().and_then(|workspace| {
            workspace
                .read(cx)
                .running
                .get(&self.id)
                .map(|running| running.view.clone())
        });
        div()
            .size_full()
            .key_context(SECONDARY_CONTEXT)
            .children(terminal)
    }
}

/// Opens the window of session `id`; a failure is reported in `main`.
fn open_session_window(
    workspace: Entity<WorkspaceView>,
    main: AnyWindowHandle,
    id: SessionId,
    title: String,
    cx: &mut App,
) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(SIZE, cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        let weak = workspace.downgrade();
        window.on_window_should_close(cx, {
            let weak = weak.clone();
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    this.detached.remove(&id);
                    cx.notify();
                })
                .ok();
                true
            }
        });
        cx.new(|cx| SessionWindow {
            workspace: weak,
            id,
            _workspace_changes: cx.observe(&workspace, |_, _, cx| cx.notify()),
            _activation: cx.observe_window_activation(
                window,
                |this: &mut SessionWindow, window, cx| {
                    if window.is_window_active() {
                        let id = this.id;
                        this.workspace
                            .update(cx, |workspace, cx| workspace.set_unseen(id, false, cx))
                            .ok();
                    }
                },
            ),
        })
    });
    match opened {
        Ok((handle, _)) => workspace.update(cx, |this, cx| {
            this.detached.insert(id, handle);
            cx.notify();
        }),
        Err(error) => {
            main.update(cx, |_, window, cx| {
                workspace.update(cx, |this, cx| this.report(error.to_string(), window, cx));
            })
            .ok();
        }
    }
}

impl WorkspaceView {
    /// Moves session `id` to a window of its own, or brings that window to
    /// the front if it is already there.
    pub(super) fn detach(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.detached.get(&id)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let Some(record) = self.workspace.session(id) else {
            return;
        };
        let title = format!(
            "{} — {}",
            self.session_name(record),
            self.place_name(record)
        );
        let (workspace, main) = (cx.entity(), window.window_handle());
        // A new window is drawn as it opens, and drawing it reads this view,
        // which cannot be read while it is being updated as it is here.
        cx.defer(move |cx| open_session_window(workspace, main, id, title, cx));
    }

    /// Closes the session's own window, showing it in the main window again.
    fn attach(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if let Some(handle) = self.detached.remove(&id) {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
        cx.notify();
    }

    /// Closes the windows of sessions that no longer exist.
    pub(super) fn prune_detached(&mut self, cx: &mut App) {
        let sessions = &self.workspace.sessions;
        let gone: Vec<SessionId> = self
            .detached
            .keys()
            .filter(|id| sessions.iter().all(|session| session.id != **id))
            .copied()
            .collect();
        for id in gone {
            if let Some(handle) = self.detached.remove(&id) {
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
        }
    }

    /// What the main window shows in place of a session that has its own window.
    pub(super) fn render_detached_notice(
        &self,
        id: SessionId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(cx.theme().muted_foreground)
            .child(t!("detached.notice"))
            .child(
                Button::new(ElementId::Name(format!("attach-{id}").into()))
                    .label(t!("detached.attach"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.attach(id, cx))),
            )
            .into_any_element()
    }
}
