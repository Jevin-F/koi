//! Uncommitted changes of every worktree, kept current by watching the
//! worktrees' files. Open changes and files panels refresh along with them.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use gpui_kit::*;
use koi_core::git;

use super::WorkspaceView;

/// Edits usually come in bursts; wait for them to settle before running
/// `git status`.
const SETTLE: Duration = Duration::from_millis(500);

impl WorkspaceView {
    /// Starts watching newly listed worktrees, stops watching ones that are
    /// gone, and recounts the changes of all of them.
    pub(super) fn track_worktrees(
        &mut self,
        previous: Vec<PathBuf>,
        current: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        for gone in previous.iter().filter(|tree| !current.contains(tree)) {
            if let Some(watcher) = &mut self.tree_watcher {
                watcher.unwatch_all_in(gone);
            }
            self.changes.remove(gone);
            self.changes_views.remove(gone);
            self.files_views.remove(gone);
        }
        if let Some(watcher) = &mut self.tree_watcher {
            for tree in &current {
                // A failed watch only means this worktree recounts on git events.
                let _ = watcher.watch(tree, true);
            }
        }
        self.mark_stale(current, cx);
    }

    /// Recounts the worktrees whose files changed. Changes inside `.git` are
    /// left to the git directory watcher.
    pub(super) fn refresh_changed_trees(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(watcher) = &self.tree_watcher else {
            return;
        };
        let changed: Vec<PathBuf> = watcher
            .take_changed()
            .into_iter()
            .filter(|path| {
                !path
                    .components()
                    .any(|part| part == Component::Normal(".git".as_ref()))
            })
            .collect();
        let trees: Vec<PathBuf> = self
            .all_trees()
            .filter(|tree| changed.iter().any(|path| path.starts_with(tree)))
            .collect();
        self.mark_stale(trees, cx);
    }

    fn all_trees(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.worktrees
            .values()
            .filter_map(|listed| listed.as_ref().ok())
            .flatten()
            .map(|tree| tree.path.clone())
    }

    /// Queues worktrees for recounting, after a short pause that lets a burst
    /// of edits settle into one `git status`.
    fn mark_stale(&mut self, trees: impl IntoIterator<Item = PathBuf>, cx: &mut Context<Self>) {
        self.stale_trees.extend(trees);
        if self.stale_trees.is_empty() || self.changes_refresh.is_some() {
            return;
        }
        self.changes_refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let Ok(trees) = this.update(cx, |this, _| std::mem::take(&mut this.stale_trees)) else {
                return;
            };
            let counted = cx
                .background_executor()
                .spawn(async move {
                    trees
                        .into_iter()
                        .map(|tree| {
                            let changes = git::changes(&tree).map_err(|error| error.to_string());
                            (tree, changes)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.changes_refresh = None;
                let known: HashSet<PathBuf> = this.all_trees().collect();
                for (tree, changes) in counted.into_iter().filter(|(tree, _)| known.contains(tree))
                {
                    if let (Some(view), Ok(files)) = (this.changes_views.get(&tree), &changes) {
                        view.update(cx, |view, cx| view.set_files(files.clone(), cx));
                    }
                    if let Some(view) = this.files_views.get(&tree) {
                        view.update(cx, |view, cx| {
                            if let Ok(files) = &changes {
                                view.set_changes(files, cx);
                            }
                            view.reload(cx);
                        });
                    }
                    this.changes.insert(tree, changes);
                }
                // Changes that arrived while counting get their own pass.
                this.mark_stale([], cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Number of uncommitted changes in a worktree, once counted.
    pub(super) fn change_count(&self, tree: &Path) -> Option<usize> {
        self.changes.get(tree)?.as_ref().ok().map(Vec::len)
    }
}
