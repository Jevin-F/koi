//! Agent activity: reading status files, inferring what Codex does, and
//! notifying the user when a session they are not looking at needs them.

use std::io;

use gpui_kit::*;
use koi_core::integration::{Integration, PI_EXTENSION, SessionReport};
use koi_core::{Activity, AppPaths, Attention, SessionId, unseen_after_change};
use koi_term::ProgramStatus;
use rust_i18n::t;

use super::WorkspaceView;
use super::resume::apply_resume;
use crate::fs_watcher::FsWatcher;

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
                if apply_resume(&mut self.workspace, &self.agents, id, &path) {
                    self.persist(window, cx);
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
                    self.set_activity(id, activity, false, window, cx);
                }
                Some(SessionReport::Shell(code)) => {
                    if !self.running.contains_key(&id) {
                        continue;
                    }
                    // The agent may have said it stopped already, with OSC 7501.
                    self.return_to_shell(id, cx);
                    if code != 0 {
                        self.notify_user(id, t!("notify.failed", code = code).into(), window, cx);
                    }
                }
                None => {}
            }
        }
    }

    /// The agent in session `id` stopped and its shell took over.
    pub(super) fn return_to_shell(&mut self, id: SessionId, cx: &mut Context<Self>) {
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        running.shell = true;
        running.reentered = None;
        running.title = None;
        running.activity = None;
        running.integration = None;
        running.guesses_from_title = false;
        running.inferred = false;
        self.set_unseen(id, false, cx);
        cx.notify();
    }

    /// Agent `agent` was started by hand in the shell of session `id`. It
    /// lacks Koi's integration arguments, so unless it reports its state in
    /// the terminal, that state is guessed from the title.
    pub(super) fn enter_agent(&mut self, id: SessionId, agent: &str, cx: &mut Context<Self>) {
        let guesses = self.agent(agent).is_some_and(|agent| {
            Integration::for_agent(agent).is_none_or(Integration::reports_activity)
        });
        let Some(running) = self.running.get_mut(&id) else {
            return;
        };
        running.shell = false;
        running.reentered = Some(agent.to_owned());
        running.activity = None;
        running.inferred = true;
        running.guesses_from_title = guesses;
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
                self.return_to_shell(id, cx);
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
            self.enter_agent(id, &agent, cx);
        }
        if let Some(running) = self.running.get_mut(&id) {
            running.guesses_from_title = false;
        }
        self.set_activity(id, activity, false, window, cx);
    }

    /// Codex reports only finished turns; submitting a prompt is taken as
    /// the start of one.
    pub(super) fn on_submitted(
        &mut self,
        id: SessionId,
        prompt: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = self.running.get(&id) else {
            return;
        };
        let (integration, activity, shell) = (running.integration, running.activity, running.shell);
        // The agent now running, which may have been started by hand.
        let is_agent = self
            .workspace
            .session(id)
            .and_then(|record| self.agent(running.reentered.as_deref().unwrap_or(&record.agent)))
            .is_some_and(|agent| !agent.command.is_empty());
        if integration == Some(Integration::Codex) && activity != Some(Activity::Working) {
            self.set_activity(id, Activity::Working, true, window, cx);
        }
        if is_agent
            && !shell
            && !matches!(integration, Some(Integration::Pi | Integration::Claude))
            && (integration.is_none() || activity == Some(Activity::Idle))
            && let Some(prompt) = prompt
            && self.workspace.auto_name_session(id, prompt)
        {
            self.persist(window, cx);
            cx.notify();
        }
    }

    /// For agents that do not report their activity, guesses it from the
    /// terminal title.
    pub(super) fn guess_activity(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = self
            .running
            .get(&id)
            .filter(|running| running.guesses_from_title)
        else {
            return;
        };
        let title = running.title.as_deref().unwrap_or_default();
        let guess = Activity::guess_from_title(title, running.activity);
        if let Some(activity) = guess.filter(|&activity| Some(activity) != running.activity) {
            self.set_activity(id, activity, true, window, cx);
        }
    }

    pub(super) fn set_activity(
        &mut self,
        id: SessionId,
        activity: Activity,
        inferred: bool,
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
        running.inferred = inferred;
        let unseen = unseen_after_change(running.unseen, previous, activity, watching);
        self.set_unseen(id, unseen, cx);
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
