//! Remembering each session's conversation, so that Koi reopens it after a
//! restart instead of starting a new one.
//!
//! Pi, Claude Code and Codex report their conversation in the session's
//! resume file. OpenCode names its terminal after the conversation, which is
//! then looked up by title.

use std::path::Path;

use gpui_kit::*;
use koi_core::history::session_titled;
use koi_core::integration::Integration;
use koi_core::{Agent, AppPaths, SessionId, Workspace};

use super::WorkspaceView;

/// Applies reports left on disk when Koi quit before the watcher handled them.
pub(super) fn recover_resumes(
    workspace: &mut Workspace,
    agents: &[Agent],
    paths: &AppPaths,
) -> bool {
    let ids: Vec<SessionId> = workspace
        .sessions
        .iter()
        .map(|session| session.id)
        .collect();
    ids.into_iter()
        .filter(|&id| apply_resume(workspace, agents, id, &paths.resume_file(id)))
        .count()
        > 0
}

/// Reads the resume file at `path` written for session `id` and remembers
/// the conversation it names. Returns whether the session changed.
pub(super) fn apply_resume(
    workspace: &mut Workspace,
    agents: &[Agent],
    id: SessionId,
    path: &Path,
) -> bool {
    let Ok(reported) = std::fs::read_to_string(path) else {
        return false;
    };
    if reported.is_empty() {
        return false;
    }
    let _ = std::fs::remove_file(path);
    let resume = workspace
        .session(id)
        .and_then(|record| agents.iter().find(|agent| agent.id == record.agent))
        .and_then(Integration::for_agent)
        .and_then(|integration| integration.resume_args(&reported));
    resume.is_some_and(|resume| workspace.set_resume(id, resume))
}

impl WorkspaceView {
    /// Looks up the conversation the terminal title of session `id` names,
    /// for agents that name their terminal after it, and remembers it.
    pub(super) fn follow_titled_session(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(record), Some(running)) = (self.workspace.session(id), self.running.get(&id))
        else {
            return;
        };
        // Only the session's own agent reopens with the arguments found.
        if running.shell
            || running
                .reentered
                .as_deref()
                .is_some_and(|agent| agent != record.agent)
        {
            return;
        }
        let Some(agent) = self.agent(&record.agent).cloned() else {
            return;
        };
        let Some(shown) = Integration::for_agent(&agent)
            .zip(running.title.as_deref())
            .and_then(|(integration, title)| integration.session_title(title))
            .map(str::to_owned)
        else {
            return;
        };
        let (shell, dir) = (self.shell.clone(), record.dir.clone());
        let found =
            cx.background_spawn(async move { session_titled(&agent, &shell, &dir, &shown) });
        cx.spawn_in(window, async move |this, cx| {
            let Some(session) = found.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if this.workspace.set_resume(id, session.resume_args) {
                    this.persist(window, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::recover_resumes;
    use koi_core::{AppPaths, Config, Workspace};

    #[test]
    fn recovers_reports_left_before_quitting() {
        let home = std::env::temp_dir().join(format!(
            "koi-resume-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::in_home(&home);
        std::fs::create_dir_all(paths.status_dir()).unwrap();
        let agents = Config::default().agents();
        let mut workspace = Workspace::default();
        workspace.add_project(home.clone());
        let pi = workspace.add_session(&home, home.clone(), "pi", Vec::new());
        let claude = workspace.add_session(&home, home.clone(), "claude", Vec::new());
        let history = home.join(".pi/task.jsonl");
        std::fs::write(paths.resume_file(pi), history.to_string_lossy().as_bytes()).unwrap();
        std::fs::write(paths.resume_file(claude), "abc-123").unwrap();

        assert!(recover_resumes(&mut workspace, &agents, &paths));
        assert_eq!(
            workspace.session(pi).unwrap().resume,
            ["--session", history.to_str().unwrap()]
        );
        assert_eq!(
            workspace.session(claude).unwrap().resume,
            ["--resume", "abc-123"]
        );
        assert!(!paths.resume_file(pi).exists());
        assert!(!recover_resumes(&mut workspace, &agents, &paths));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
