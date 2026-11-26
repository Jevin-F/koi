//! What the services window and the status bar know of background
//! services: who owns them and how many there are.

use gpui_kit::*;
use koi_core::SessionId;
use koi_core::services::{Group, Owner, Source, Tag};
use rust_i18n::t;

use super::{WorkspaceView, directory_name};

/// How a group of services is introduced: a title and, for a session, where
/// it works.
pub struct SourceLabel {
    pub title: String,
    pub place: Option<String>,
}

impl WorkspaceView {
    /// Session `id` as its processes are found, if it was started.
    pub(super) fn owner(&self, id: SessionId, cx: &App) -> Option<Owner> {
        let running = self.running.get(&id)?;
        Some(Owner {
            tag: Tag::own(id),
            root: running.view.read(cx).pid(),
        })
    }

    pub(super) fn owners(&self, cx: &App) -> Vec<Owner> {
        self.running
            .keys()
            .filter_map(|id| self.owner(*id, cx))
            .collect()
    }

    /// Background services as last sampled, by session.
    pub(crate) fn services(&self) -> &[Group] {
        &self.services
    }

    /// Samples now for the services window, unless the main window's own
    /// sampling runs.
    pub(crate) fn sample_services(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sampler.is_none() {
            self.sample(window, cx).detach();
        }
    }

    /// Samples again now, so stopped services leave the list at once.
    pub(crate) fn resample(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sample(window, cx).detach();
    }

    pub(crate) fn source_label(&self, source: Source) -> SourceLabel {
        let Source::Session(id) = source else {
            return SourceLabel {
                title: t!("services.leftover").into_owned(),
                place: None,
            };
        };
        match self.workspace.session(id) {
            Some(record) => SourceLabel {
                title: self.session_name(record),
                place: Some(format!(
                    "{} · {}",
                    directory_name(&record.project),
                    self.place_name(record)
                )),
            },
            None => SourceLabel {
                title: t!("services.closed_session").into_owned(),
                place: None,
            },
        }
    }

    /// How many background services were last found.
    pub(super) fn service_count(&self) -> usize {
        self.services.iter().map(|group| group.services.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext};
    use koi_core::resources::Usage;
    use koi_core::services::{Group, Service, Source};

    #[gpui_kit::test]
    fn the_services_window_follows_the_workspace_without_taking_it_over(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let home = std::env::temp_dir().join(format!("koi-services-test-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let paths = koi_core::AppPaths::in_home(&home);
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::app_menu::init(cx);
            crate::terminal::init(cx);
            crate::workspace::init(cx);
            crate::main_window::init(cx);
            crate::settings::init(&paths, cx);
            crate::main_window::open(paths, Vec::new(), cx);
            crate::services_view::open(cx);
        });
        let [main, services] = cx.update(|cx| cx.windows())[..] else {
            panic!("expected the main and the services window");
        };
        let workspace = cx.update(|cx| crate::main_window::workspace(cx).unwrap());
        cx.update_window(services, |_, window, cx| window.render_frame(cx))
            .unwrap();
        // Updates of the workspace still run in the main window.
        let current = cx
            .update(|cx| cx.with_window(workspace.entity_id(), |window, _| window.window_handle()));
        assert_eq!(current, Some(main));

        workspace.update(cx, |workspace, cx| {
            workspace.services = vec![Group {
                source: Source::Leftover,
                services: vec![Service {
                    pid: 999_999,
                    command: "node server.js".into(),
                    ports: vec![3000],
                    usage: Usage::default(),
                }],
            }];
            cx.notify();
        });
        cx.update_window(services, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("stop-service", 999_999usize)).is_some());
            window.press("cmd-w", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| cx.windows()), [main]);
        std::fs::remove_dir_all(home).unwrap();
    }
}
