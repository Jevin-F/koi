//! Sessions with news the user has not looked at: a finished turn or a
//! question. They are marked in the sidebar and counted on the Dock icon.

use gpui_kit::*;
use koi_core::SessionId;

use super::WorkspaceView;
use crate::dock_badge;

impl WorkspaceView {
    /// Whether the user is looking at session `id`: on screen in the active
    /// main window, or in its own window while that one is active.
    pub(super) fn watching(&self, id: SessionId, window: &Window, cx: &mut App) -> bool {
        if window.is_window_active() && self.workspace.on_screen().any(|shown| shown == id) {
            return true;
        }
        self.detached.get(&id).is_some_and(|handle| {
            handle
                .update(cx, |_, window, _| window.is_window_active())
                .unwrap_or(false)
        })
    }

    /// Whether session `id` has news the user has not looked at.
    pub(super) fn is_unseen(&self, id: SessionId) -> bool {
        self.running.get(&id).is_some_and(|running| running.unseen)
    }

    /// Clears the mark of every session the user is now looking at in the
    /// main window.
    pub(super) fn mark_seen(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            return;
        }
        let shown: Vec<SessionId> = self.workspace.on_screen().collect();
        for id in shown {
            self.set_unseen(id, false, cx);
        }
    }

    /// Marks or clears session `id`, keeping the Dock count in step.
    pub(super) fn set_unseen(&mut self, id: SessionId, unseen: bool, cx: &mut Context<Self>) {
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        if running.unseen != unseen {
            running.unseen = unseen;
            self.update_badge();
            cx.notify();
        }
    }

    /// Shows the number of unseen sessions on the Dock icon.
    pub(super) fn update_badge(&mut self) {
        let count = self
            .running
            .values()
            .filter(|running| running.unseen)
            .count();
        if count != self.badge {
            self.badge = count;
            dock_badge::show(count);
        }
    }
}
