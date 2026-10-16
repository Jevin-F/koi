//! The latest reply of each agent session, read from the conversation the
//! agent saves, so the overview shows what the agent said rather than the
//! last line its screen happens to show.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::*;
use koi_core::history::reads_replies;
use koi_core::integration::Integration;
use koi_core::{SessionId, SessionRecord};

use super::WorkspaceView;

/// What reading one session's latest reply takes.
struct Lookup {
    id: SessionId,
    integration: Integration,
    dir: PathBuf,
    resume: Vec<String>,
}

impl WorkspaceView {
    /// The agent whose saved conversation summarizes `record`: the session's
    /// own agent while it runs, if Koi can read that agent's replies.
    pub(super) fn reply_source(&self, record: &SessionRecord) -> Option<Integration> {
        let running = self.running.get(&record.id)?;
        if running.shell || running.reentered.is_some() {
            return None;
        }
        Integration::for_agent(self.agent(&record.agent)?)
            .filter(|&integration| reads_replies(integration))
    }

    /// Reads the latest replies in the background, re-reading only the
    /// conversations whose files changed since the last time.
    pub(super) fn refresh_replies(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let lookups: Vec<Lookup> = self
            .workspace
            .sessions
            .iter()
            .filter_map(|record| {
                Some(Lookup {
                    id: record.id,
                    integration: self.reply_source(record)?,
                    dir: record.dir.clone(),
                    resume: record.resume.clone(),
                })
            })
            .collect();
        let home = self.paths.home().to_owned();
        let mut cache = std::mem::take(&mut self.reply_cache);
        cx.spawn(async move |this, cx| {
            let (cache, replies) = cx
                .background_executor()
                .spawn(async move {
                    let replies: HashMap<SessionId, String> = lookups
                        .iter()
                        .filter_map(|lookup| {
                            let reply = cache.latest(
                                lookup.integration,
                                &home,
                                &lookup.dir,
                                &lookup.resume,
                            )?;
                            Some((lookup.id, reply))
                        })
                        .collect();
                    (cache, replies)
                })
                .await;
            this.update(cx, |this, cx| {
                this.reply_cache = cache;
                if this.replies != replies {
                    this.replies = replies;
                    cx.notify();
                }
            })
            .ok();
        })
    }
}
