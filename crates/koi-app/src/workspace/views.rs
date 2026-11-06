//! What the main area shows for a session: its terminal, or the changes or
//! files of its worktree. The panels are created when first shown and are
//! shared by sessions in the same worktree.

use gpui_kit::*;
use koi_core::SessionId;

use super::changes_view::ChangesView;
use super::files_view::FilesView;
use super::{ToggleChanges, ToggleFiles, WorkspaceView};

/// A panel shown instead of a session's terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Changes,
    Files,
}

impl WorkspaceView {
    /// Whether closing the app would discard an editor's in-memory changes.
    pub(crate) fn has_unsaved_files(&self, cx: &App) -> bool {
        self.files_views
            .values()
            .any(|view| view.read(cx).has_unsaved())
    }

    /// Whether a worktree has unsaved editor changes.
    pub(super) fn has_unsaved_in(&self, dir: &std::path::Path, cx: &App) -> bool {
        self.files_views
            .get(dir)
            .is_some_and(|view| view.read(cx).has_unsaved())
    }

    /// The panel the active session shows; `None` for its terminal.
    pub(super) fn active_panel(&self) -> Option<Panel> {
        self.panels.get(&self.workspace.active?).copied()
    }

    /// Shows `panel` for the active session, or its terminal for `None`. A
    /// plain folder has no changes panel.
    pub(super) fn show_panel(
        &mut self,
        panel: Option<Panel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self
            .workspace
            .active
            .and_then(|id| self.workspace.session(id))
            .cloned()
        else {
            return;
        };
        if panel == Some(Panel::Changes) && self.is_folder(&record.project) {
            return;
        }
        match panel {
            None => {
                self.panels.remove(&record.id);
            }
            Some(Panel::Changes) => {
                let branch = self.place_name(&record);
                let files = self
                    .changes
                    .get(&record.dir)
                    .and_then(|changes| changes.as_ref().ok())
                    .cloned()
                    .unwrap_or_default();
                let layout = self.changes_layout.clone();
                let view = self
                    .changes_views
                    .entry(record.dir.clone())
                    .or_insert_with(|| {
                        cx.new(|cx| {
                            ChangesView::new(record.dir.clone(), branch.clone(), layout, window, cx)
                        })
                    })
                    .clone();
                view.update(cx, |view, cx| {
                    view.set_branch(branch);
                    view.set_files(files, cx);
                });
                self.panels.insert(record.id, Panel::Changes);
            }
            Some(Panel::Files) => {
                self.files_view(&record.dir, window, cx);
                self.panels.insert(record.id, Panel::Files);
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn files_view(
        &mut self,
        dir: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<FilesView> {
        if let Some(view) = self.files_views.get(dir) {
            return view.clone();
        }
        let layout = self.files_layout.clone();
        let view = cx.new(|cx| {
            let mut view = FilesView::new(dir.to_owned(), layout, window, cx);
            if let Some(Ok(changes)) = self.changes.get(dir) {
                view.set_changes(changes, cx);
            }
            view
        });
        self.files_views.insert(dir.to_owned(), view.clone());
        view
    }

    /// Shows `panel`, or goes back to the terminal if it is already shown.
    fn toggle_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        let next = (self.active_panel() != Some(panel)).then_some(panel);
        self.show_panel(next, window, cx);
    }

    pub(super) fn toggle_changes(
        &mut self,
        _: &ToggleChanges,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_panel(Panel::Changes, window, cx);
    }

    pub(super) fn toggle_files(
        &mut self,
        _: &ToggleFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_panel(Panel::Files, window, cx);
    }

    /// Opens `path` of the session's worktree in the files panel.
    pub(super) fn open_file(
        &mut self,
        id: SessionId,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = self.workspace.session(id).map(|record| record.dir.clone()) else {
            return;
        };
        self.workspace.open(id);
        let view = self.files_view(&dir, window, cx);
        view.update(cx, |view, cx| view.reveal(path, window, cx));
        self.panels.insert(id, Panel::Files);
        self.after_tab_change(window, cx);
    }

    /// The active session's panel element, if it shows one.
    pub(super) fn render_active_panel(&self) -> Option<AnyElement> {
        let dir = &self.workspace.session(self.workspace.active?)?.dir;
        match self.active_panel()? {
            Panel::Changes => self
                .changes_views
                .get(dir)
                .cloned()
                .map(IntoElement::into_any_element),
            Panel::Files => self
                .files_views
                .get(dir)
                .cloned()
                .map(IntoElement::into_any_element),
        }
    }

    /// Forgets panels of sessions that no longer exist.
    pub(super) fn prune_panels(&mut self) {
        let sessions = &self.workspace.sessions;
        self.panels
            .retain(|id, _| sessions.iter().any(|session| session.id == *id));
    }
}
