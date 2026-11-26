//! Ending sessions together with the servers and watchers their agents
//! started in the background, which outlive the terminal otherwise: when a
//! session is closed, and when Koi quits.

use gpui_kit::*;
use koi_core::SessionId;
use koi_core::process_stop::{self, GRACE, Target};
use koi_core::resources::ProcessTable;
use koi_core::services::session_roots;
use rust_i18n::t;

use super::WorkspaceView;

impl WorkspaceView {
    /// Closes the terminals of sessions `ids` and stops every process they
    /// started, in the background.
    pub(super) fn end_sessions(
        &mut self,
        ids: Vec<SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Listed before the terminals close, while the processes still run
        // below them.
        let targets = self.targets(&ids, cx);
        for id in &ids {
            self.running.remove(id);
        }
        let stopped = cx.background_spawn(async move { process_stop::stop(&targets?, GRACE) });
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = stopped.await {
                this.update_in(cx, |this, window, cx| {
                    this.report(t!("error.stop", error = error), window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Stops the processes of every session when Koi quits.
    pub(super) fn end_sessions_on_quit(cx: &mut Context<Self>) {
        cx.on_app_quit(|this, cx| {
            let ids: Vec<SessionId> = this.running.keys().copied().collect();
            // Nothing is left to report a failure in.
            let _ = this
                .targets(&ids, cx)
                .and_then(|targets| process_stop::stop(&targets, GRACE));
            async {}
        })
        .detach();
    }

    /// The processes of sessions `ids` as they run now: those below their
    /// terminals and those that left them but carry their tag.
    fn targets(&self, ids: &[SessionId], cx: &App) -> std::io::Result<Vec<Target>> {
        let owners: Vec<_> = ids.iter().filter_map(|id| self.owner(*id, cx)).collect();
        if owners.is_empty() {
            return Ok(Vec::new());
        }
        let table = ProcessTable::read()?;
        let roots: Vec<u32> = owners
            .into_iter()
            .flat_map(|owner| session_roots(&table, owner))
            .collect();
        Ok(process_stop::targets(&table, &roots))
    }
}
