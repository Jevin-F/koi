//! Agent activity: reading status files and terminal reports, telling it
//! from the output of agents that report none, and notifying the user when
//! a session they are not looking at needs them.

use std::io;
use std::time::{Duration, Instant, SystemTime};

use gpui_kit::*;
use koi_core::integration::{Integration, PI_EXTENSION, SessionReport};
use koi_core::{Activity, AppPaths, Attention, SessionId, unseen_after_change};
use koi_term::ProgramStatus;
use rust_i18n::t;

use super::WorkspaceView;
use super::resume::apply_resume;
use crate::fs_watcher::FsWatcher;

/// How often agents that report no activity have their output checked.
const OUTPUT_CHECK: Duration = Duration::from_secs(1);
/// A prompt names the session only if the agent starts working this soon.
const PROMPT_STARTS_WORK: Duration = Duration::from_secs(10);

/// Installs the files integrations load, updating them if Koi changed, and
/// starts watching the status files they write.
pub(super) fn watch_status_files(paths: &AppPaths) -> io::Result<FsWatcher> {
    let extension = paths.pi_extension_file();
    if std::fs::read_to_string(&extension).ok().as_deref() != Some(PI_EXTENSION) {
        if let Some(dir) = extension.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&extension, PI_EXTENSION)?;
    }
    std::fs::create_dir_all(paths.status_dir())?;
    let mut watcher = FsWatcher::new()?;
    watcher.watch(&paths.status_dir(), false)?;
    Ok(watcher)
}

/// Clicking a notification shows the session it is about.
pub(super) fn handle_notification_clicks(window: &mut Window, cx: &mut Context<WorkspaceView>) {
    let view = cx.entity().downgrade();
    let handle = window.window_handle();
    cx.on_system_notification_response(move |response, cx| {
        let Ok(id) = response.tag.parse::<SessionId>() else {
            return;
        };
        cx.activate(true);
        handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                view.update(cx, |this, cx| this.open(id, window, cx)).ok();
            })
            .ok();
    });
}

impl WorkspaceView {
    pub(super) fn read_status_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(watcher) = &self.status_watcher else {
            return;
        };
        for path in watcher.take_changed() {
            if let Some(id) = AppPaths::session_of_resume_file(&path) {
                let resumed = apply_resume(&mut self.workspace, &self.agents, id, &path);
                // Codex may have titled or renamed the conversation since.
                let renamed = self.name_after_agent_title(id);
                if resumed || renamed {
                    self.persist(window, cx);
                }
                if renamed {
                    cx.notify();
                }
                continue;
            }
            if let Some(id) = AppPaths::session_of_title_file(&path) {
                if let Ok(prompt) = std::fs::read_to_string(&path)
                    && !prompt.is_empty()
                {
                    if self.workspace.auto_name_session(id, &prompt) {
                        self.persist(window, cx);
                        cx.notify();
                    }
                    let _ = std::fs::remove_file(&path);
                }
                continue;
            }
            let Some(id) = AppPaths::session_of_status_file(&path) else {
                continue;
            };
            let contents = std::fs::read_to_string(&path).unwrap_or_default();
            match SessionReport::parse(&contents) {
                Some(SessionReport::Activity(activity)) => {
                    self.set_activity(id, activity, window, cx);
                }
                Some(SessionReport::Shell(code)) => {
                    if !self.running.contains_key(&id) {
                        continue;
                    }
                    // The agent may have said it stopped already, with OSC 7501.
                    self.return_to_shell(id, window, cx);
                    if code != 0 {
                        self.notify_user(id, t!("notify.failed", code = code).into(), window, cx);
                    }
                }
                None => {}
            }
        }
    }

    /// The agent in session `id` stopped and its shell took over, which a
    /// restart of Koi then reopens.
    pub(super) fn return_to_shell(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        running.shell = true;
        running.reentered = None;
        running.entered = None;
        running.title = None;
        running.activity = None;
        running.integration = None;
        running.watches_output = false;
        running.prompt = None;
        self.set_unseen(id, false, cx);
        if self.workspace.set_at_shell(id, true) {
            self.persist(window, cx);
        }
        cx.notify();
    }

    /// Agent `agent` was started by hand in the shell of session `id`, which
    /// a restart of Koi then runs. It lacks Koi's integration arguments, so
    /// unless it reports its state in the terminal, that state is told from
    /// its output, and its conversation is looked up once it finishes a turn.
    pub(super) fn enter_agent(
        &mut self,
        id: SessionId,
        agent: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        running.shell = false;
        running.reentered = Some(agent.to_owned());
        running.entered = Some(SystemTime::now());
        running.activity = None;
        running.watches_output = true;
        if self.workspace.enter_agent(id, agent) {
            self.persist(window, cx);
        }
        cx.notify();
    }

    /// A program in the terminal of session `id` reported its state with
    /// OSC 7501, as Pi does whether or not Koi started it.
    pub(super) fn on_program_status(
        &mut self,
        id: SessionId,
        status: &ProgramStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = self.running.get(&id) else {
            return;
        };
        let Some(activity) = Activity::from_program_state(&status.state) else {
            // `clear`: the program stopped, so the shell is back.
            if status.state == "clear" && !running.shell {
                self.return_to_shell(id, window, cx);
            }
            return;
        };
        if running.shell {
            // Started by hand: the report names the program.
            let Some(agent) = status
                .app
                .as_deref()
                .and_then(|app| {
                    self.agents
                        .iter()
                        .find(|agent| agent.program_name() == Some(app))
                })
                .map(|agent| agent.id.clone())
            else {
                return;
            };
            self.enter_agent(id, &agent, window, cx);
        }
        if let Some(running) = self.running.get_mut(&id) {
            running.watches_output = false;
        }
        self.set_activity(id, activity, window, cx);
    }

    /// Remembers a prompt the user submitted, to name the session once the
    /// agent starts working on it: menu choices and confirmations start no
    /// work, so they name nothing. Pi and Claude Code report their prompts.
    pub(super) fn on_submitted(&mut self, id: SessionId, prompt: Option<&str>) {
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        let reports_prompt = matches!(
            running.integration,
            Some(Integration::Pi | Integration::Claude)
        );
        running.prompt = prompt
            .filter(|_| !running.shell && !reports_prompt)
            .map(|prompt| (prompt.to_owned(), Instant::now()));
    }

    /// Starts updating, every second, the activity of agents that report
    /// none, from their terminal output.
    pub(super) fn watch_output(window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(OUTPUT_CHECK).await;
                if this
                    .update_in(cx, |this, window, cx| this.check_output(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn check_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let changed: Vec<(SessionId, Activity)> = self
            .running
            .iter()
            .filter(|(_, running)| running.watches_output && !running.shell)
            .filter_map(|(&id, running)| {
                let terminal = running.view.read(cx);
                if terminal.exit_code().is_some() {
                    return None;
                }
                let activity = match terminal.busy()? {
                    true => Activity::Working,
                    false => Activity::Idle,
                };
                (running.activity != Some(activity)).then_some((id, activity))
            })
            .collect();
        for (id, activity) in changed {
            self.set_activity(id, activity, window, cx);
        }
    }

    pub(super) fn set_activity(
        &mut self,
        id: SessionId,
        activity: Activity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let watching = self.watching(id, window, cx);
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        if running.shell {
            return;
        }
        let previous = running.activity.replace(activity);
        // A finished turn of an agent started by hand has saved its conversation.
        let turn_saved = running.entered.is_some()
            && previous == Some(Activity::Working)
            && activity != Activity::Working;
        // A prompt the agent started working on names the session.
        let prompt = (activity == Activity::Working)
            .then(|| running.prompt.take())
            .flatten()
            .filter(|(_, at)| at.elapsed() < PROMPT_STARTS_WORK);
        let unseen = unseen_after_change(running.unseen, previous, activity, watching);
        self.set_unseen(id, unseen, cx);
        if turn_saved {
            self.adopt_entered_conversation(id, window, cx);
        }
        if let Some((prompt, _)) = prompt
            && self.workspace.auto_name_session(id, &prompt)
        {
            self.persist(window, cx);
        }
        cx.notify();

        let Some(attention) = Attention::of_change(previous, activity) else {
            return;
        };
        let body = match attention {
            Attention::Waiting => t!("notify.waiting"),
            Attention::Finished => t!("notify.finished"),
        };
        self.notify_user(id, body.into(), window, cx);
    }

    /// Tells the user when a process fails, since it needs a restart.
    pub(super) fn on_exited(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        self.usage.remove(&id);
        self.set_unseen(id, false, cx);
        let code = self
            .running
            .get(&id)
            .and_then(|running| running.view.read(cx).exit_code().flatten());
        if let Some(code) = code.filter(|&code| code != 0) {
            self.notify_user(id, t!("notify.failed", code = code).into(), window, cx);
        }
    }

    /// Posts a system notification about session `id`, unless the user is
    /// looking at it.
    fn notify_user(&self, id: SessionId, body: SharedString, window: &Window, cx: &mut App) {
        if self.watching(id, window, cx) {
            return;
        }
        let Some(record) = self.workspace.session(id) else {
            return;
        };
        cx.show_system_notification(SystemNotification {
            tag: id.to_string().into(),
            title: format!(
                "{} · {}",
                self.session_name(record),
                self.place_name(record)
            )
            .into(),
            body,
            actions: Vec::new(),
        });
    }
}
