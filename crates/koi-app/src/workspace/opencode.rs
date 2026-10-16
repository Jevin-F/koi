//! OpenCode sessions take their activity from OpenCode's background service,
//! asked every few seconds about the conversations they show.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::*;
use koi_core::integration::Integration;
use koi_core::{Activity, Agent, SessionId, opencode};

use super::WorkspaceView;

const INTERVAL: Duration = Duration::from_secs(2);

/// An OpenCode session to ask about: its agent and conversation.
struct Watched {
    session: SessionId,
    agent: Agent,
    conversation: String,
}

impl WorkspaceView {
    /// Asks OpenCode's service about its sessions, for as long as the view
    /// lives. Programs are located once per agent, as that starts a shell.
    pub(super) fn poll_opencode(window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            let mut programs: HashMap<String, Option<PathBuf>> = HashMap::new();
            loop {
                cx.background_executor().timer(INTERVAL).await;
                let Ok((watched, shell)) =
                    this.update(cx, |this, _| (this.opencode_sessions(), this.shell.clone()))
                else {
                    break;
                };
                if watched.is_empty() {
                    continue;
                }
                let (found, known) = cx
                    .background_spawn(async move {
                        let found = ask(&watched, &mut programs, &shell);
                        (found, programs)
                    })
                    .await;
                programs = known;
                let updated = this.update_in(cx, |this, window, cx| {
                    for (session, activity) in found {
                        let Some(running) = this.running.get_mut(&session) else {
                            continue;
                        };
                        // The service knows better than the output.
                        running.watches_output = false;
                        if running.activity != Some(activity) {
                            this.set_activity(session, activity, window, cx);
                        }
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        })
    }

    /// Running OpenCode sessions whose conversation is known.
    fn opencode_sessions(&self) -> Vec<Watched> {
        self.workspace
            .sessions
            .iter()
            .filter(|record| {
                self.running.get(&record.id).is_some_and(|running| {
                    !running.shell
                        && running
                            .reentered
                            .as_ref()
                            .is_none_or(|agent| *agent == record.agent)
                })
            })
            .filter_map(|record| {
                let agent = self.agent(&record.agent)?;
                let conversation = Integration::for_agent(agent)
                    .filter(|&integration| integration == Integration::OpenCode)?
                    .conversation_id(&record.resume)?;
                Some(Watched {
                    session: record.id,
                    agent: agent.clone(),
                    conversation: conversation.to_owned(),
                })
            })
            .collect()
    }
}

/// The activity of each watched session, asking once per OpenCode agent.
fn ask(
    watched: &[Watched],
    programs: &mut HashMap<String, Option<PathBuf>>,
    shell: &str,
) -> Vec<(SessionId, Activity)> {
    let mut asked: HashMap<&str, HashMap<String, Activity>> = HashMap::new();
    for item in watched {
        let agent = &item.agent;
        if asked.contains_key(agent.id.as_str()) {
            continue;
        }
        let conversations: Vec<String> = watched
            .iter()
            .filter(|other| other.agent.id == agent.id)
            .map(|other| other.conversation.clone())
            .collect();
        let program = programs
            .entry(agent.id.clone())
            .or_insert_with(|| agent.locate(shell));
        let activities = program
            .as_deref()
            .map(|program| opencode::activities(program, &conversations))
            .unwrap_or_default();
        asked.insert(&agent.id, activities);
    }
    watched
        .iter()
        .filter_map(|item| {
            let activity = asked.get(item.agent.id.as_str())?.get(&item.conversation)?;
            Some((item.session, *activity))
        })
        .collect()
}
