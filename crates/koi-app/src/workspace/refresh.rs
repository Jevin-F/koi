//! Keeps worktree lists current. Each project's shared git directory is
//! watched, so worktrees added, removed or switched to another branch
//! outside Koi show up without a refresh button.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use koi_core::git::{self, Worktree};

use super::WorkspaceView;

impl WorkspaceView {
    /// Lists the project's worktrees again, makes sure its git directory is
    /// watched, and has the changes of its worktrees recounted.
    pub(super) fn refresh_worktrees(&mut self, project: &Path, cx: &mut Context<Self>) {
        let paths = |listed: Option<&Result<Vec<Worktree>, String>>| -> Vec<PathBuf> {
            listed
                .and_then(|listed| listed.as_ref().ok())
                .map(|trees| trees.iter().map(|tree| tree.path.clone()).collect())
                .unwrap_or_default()
        };
        let previous = paths(self.worktrees.get(project));
        let listed = git::worktrees(project).map_err(|error| error.to_string());
        self.worktrees.insert(project.to_owned(), listed);
        let current = paths(self.worktrees.get(project));
        for tree in &current {
            if let Some(view) = self.changes_views.get(tree) {
                let branch = self.place_of(project, tree);
                view.update(cx, |view, _| view.set_branch(branch));
            }
        }
        self.track_worktrees(previous, current, cx);
        self.watch_git_dir(project);
    }

    fn watch_git_dir(&mut self, project: &Path) {
        let Some(watcher) = &mut self.git_watcher else {
            return;
        };
        let common = match self.git_dirs.get(project) {
            Some(common) => common.clone(),
            None => match git::common_dir(project) {
                Ok(common) => {
                    self.git_dirs.insert(project.to_owned(), common.clone());
                    common
                }
                Err(_) => return,
            },
        };
        // HEAD and the worktree list live at the top; each linked worktree
        // keeps its HEAD under `worktrees/`, which appears with the first one.
        // A failed watch only means this project refreshes on Koi's own changes.
        let _ = watcher.watch(&common, false);
        let _ = watcher.watch(&common.join("worktrees"), true);
    }

    pub(super) fn forget_git_dir(&mut self, project: &Path) {
        if let (Some(common), Some(watcher)) =
            (self.git_dirs.remove(project), &mut self.git_watcher)
        {
            watcher.unwatch_all_in(&common);
        }
    }

    pub(super) fn refresh_changed_projects(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(watcher) = &self.git_watcher else {
            return;
        };
        let changed = watcher.take_changed();
        let projects: Vec<_> = self
            .git_dirs
            .iter()
            .filter(|(_, common)| changed.iter().any(|path| path.starts_with(common)))
            .map(|(project, _)| project.clone())
            .collect();
        for project in &projects {
            self.refresh_worktrees(project, cx);
        }
        if !projects.is_empty() {
            cx.notify();
        }
    }
}
