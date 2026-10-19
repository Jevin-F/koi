//! The user's workspace: added projects and the sessions running in them.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::layout::SidebarWidth;
use crate::session_label;

/// Identifies a session; stable across restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(u64);

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = std::num::ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(Self)
    }
}

/// A directory added by the user: a git repository or a plain folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub root: PathBuf,
    /// Left out of the sidebar; its sessions keep running.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl Project {
    /// Display name: the directory's name.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map_or_else(
                || self.root.to_string_lossy(),
                |name| name.to_string_lossy(),
            )
            .into_owned()
    }
}

/// An agent running in a directory of a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: SessionId,
    pub project: PathBuf,
    /// User-visible task name; absent for sessions that have not been named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Set once the user names the session, even back to the agent name;
    /// messages then no longer name it automatically.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub named_by_user: bool,
    /// Working directory: the project root or one of its worktrees.
    pub dir: PathBuf,
    /// [`crate::Agent::id`] of the program to run.
    pub agent: String,
    /// Arguments that resume an earlier conversation; empty for a new one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resume: Vec<String>,
    /// The agent had exited and the shell had taken over; a restart opens
    /// the shell instead of the agent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub at_shell: bool,
}

/// Where a split shows its session, relative to the active tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitSide {
    Right,
    Below,
}

/// A second session shown beside the active tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub session: SessionId,
    pub side: SplitSide,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    /// Manual sidebar width; window resizing does not overwrite it.
    pub sidebar_width: SidebarWidth,
    pub projects: Vec<Project>,
    /// In creation order.
    pub sessions: Vec<SessionRecord>,
    /// Sessions opened as tabs, in tab order.
    pub tabs: Vec<SessionId>,
    /// The same tabs from most to least recently shown; closing the active
    /// tab shows the first one left.
    recent: Vec<SessionId>,
    /// The tab on screen; `None` shows the overview.
    pub active: Option<SessionId>,
    /// Never the active session, which is shown once only.
    pub split: Option<Split>,
}

impl Workspace {
    /// Adds a project, or shows it again if it is hidden; returns `false`
    /// if it is already shown.
    pub fn add_project(&mut self, root: PathBuf) -> bool {
        if self.projects.iter().any(|project| project.root == root) {
            return self.set_project_hidden(&root, false);
        }
        self.projects.push(Project {
            root,
            hidden: false,
        });
        true
    }

    /// Hides a project from the sidebar or shows it again; returns whether
    /// anything changed.
    pub fn set_project_hidden(&mut self, root: &Path, hidden: bool) -> bool {
        match self
            .projects
            .iter_mut()
            .find(|project| project.root == root)
        {
            Some(project) if project.hidden != hidden => {
                project.hidden = hidden;
                true
            }
            _ => false,
        }
    }

    /// Sessions of projects that are not hidden, in creation order.
    pub fn shown_sessions(&self) -> impl Iterator<Item = &SessionRecord> {
        self.sessions.iter().filter(|session| {
            !self
                .projects
                .iter()
                .any(|project| project.hidden && project.root == session.project)
        })
    }

    /// Shows every hidden project; returns whether any was hidden.
    pub fn show_all_projects(&mut self) -> bool {
        let mut changed = false;
        for project in self.projects.iter_mut().filter(|project| project.hidden) {
            project.hidden = false;
            changed = true;
        }
        changed
    }

    /// Removes a project and its sessions, returning the removed sessions.
    pub fn remove_project(&mut self, root: &Path) -> Vec<SessionId> {
        self.projects.retain(|project| project.root != root);
        self.remove_sessions(|session| session.project == root)
    }

    /// Adds a session and opens it.
    pub fn add_session(
        &mut self,
        project: &Path,
        dir: PathBuf,
        agent: &str,
        resume: Vec<String>,
    ) -> SessionId {
        let next = self.sessions.iter().map(|session| session.id.0 + 1).max();
        let id = SessionId(next.unwrap_or(1));
        self.sessions.push(SessionRecord {
            id,
            project: project.to_owned(),
            label: None,
            named_by_user: false,
            dir,
            agent: agent.to_owned(),
            resume,
            at_shell: false,
        });
        self.open(id);
        id
    }

    /// Sessions the main window shows: the active tab and the split beside
    /// it. The overview shows neither.
    pub fn on_screen(&self) -> impl Iterator<Item = SessionId> {
        let split = self.split.filter(|_| self.active.is_some());
        self.active
            .into_iter()
            .chain(split.map(|split| split.session))
    }

    pub fn session(&self, id: SessionId) -> Option<&SessionRecord> {
        self.sessions.iter().find(|session| session.id == id)
    }

    /// Names a session; a blank name restores its agent's default label.
    pub fn rename_session(&mut self, id: SessionId, name: &str) -> bool {
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return false;
        };
        let name = session_label::bounded(name.trim());
        session.label = (!name.is_empty()).then_some(name);
        session.named_by_user = true;
        true
    }

    /// Uses a submitted message as the default name, unless already named
    /// automatically or by the user.
    pub fn auto_name_session(&mut self, id: SessionId, message: &str) -> bool {
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return false;
        };
        if session.named_by_user || session.label.is_some() {
            return false;
        }
        let Some(label) = session_label::from_message(message) else {
            return false;
        };
        session.label = Some(label);
        true
    }

    /// Remembers the arguments that reopen the session's conversation after
    /// a restart; returns whether they changed.
    pub fn set_resume(&mut self, id: SessionId, resume: Vec<String>) -> bool {
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return false;
        };
        if session.resume == resume {
            return false;
        }
        session.resume = resume;
        true
    }

    /// The user started `agent` by hand in the session's shell; a restart
    /// runs it, resuming the session's conversation only if that was the
    /// same agent's. Returns whether anything changed.
    pub fn enter_agent(&mut self, id: SessionId, agent: &str) -> bool {
        let Some(session) = self.session(id) else {
            return false;
        };
        let resume = if session.agent == agent {
            session.resume.clone()
        } else {
            Vec::new()
        };
        self.adopt_conversation(id, agent, resume)
    }

    /// Makes `agent`, resuming the conversation `resume`, the program the
    /// session restarts with, as when the user started that agent by hand in
    /// the session's shell. Returns whether anything changed.
    pub fn adopt_conversation(&mut self, id: SessionId, agent: &str, resume: Vec<String>) -> bool {
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return false;
        };
        if session.agent == agent && session.resume == resume && !session.at_shell {
            return false;
        }
        session.agent = agent.to_owned();
        session.resume = resume;
        session.at_shell = false;
        true
    }

    /// Records whether the session's agent has exited to the shell, which a
    /// restart then reopens. Returns whether it changed.
    pub fn set_at_shell(&mut self, id: SessionId, at_shell: bool) -> bool {
        match self.sessions.iter_mut().find(|session| session.id == id) {
            Some(session) if session.at_shell != at_shell => {
                session.at_shell = at_shell;
                true
            }
            _ => false,
        }
    }

    /// Drops tabs of sessions that no longer exist and makes sure the active
    /// session has a tab, as saved state may come from an older version.
    pub fn repair(&mut self) {
        let sessions: Vec<SessionId> = self.sessions.iter().map(|session| session.id).collect();
        self.tabs.retain(|tab| sessions.contains(tab));
        match self.active {
            Some(id) if !sessions.contains(&id) => self.active = self.tabs.first().copied(),
            Some(id) if !self.tabs.contains(&id) => self.tabs.push(id),
            _ => {}
        }
        // Older state has no recency; unknown tabs count as least recent.
        self.recent.retain(|tab| self.tabs.contains(tab));
        for &tab in &self.tabs {
            if !self.recent.contains(&tab) {
                self.recent.push(tab);
            }
        }
        if let Some(id) = self.active {
            self.mark_recent(id);
        }
        let stale =
            |split: Split| !sessions.contains(&split.session) || Some(split.session) == self.active;
        if self.split.is_some_and(stale) {
            self.split = None;
        }
    }

    /// Shows a session, adding a tab for it at the end if it has none.
    pub fn open(&mut self, id: SessionId) {
        if !self.tabs.contains(&id) {
            self.tabs.push(id);
        }
        self.mark_recent(id);
        self.active = Some(id);
        if self.split.is_some_and(|split| split.session == id) {
            self.split = None;
        }
    }

    fn mark_recent(&mut self, id: SessionId) {
        self.recent.retain(|&tab| tab != id);
        self.recent.insert(0, id);
    }

    /// Moves tab `id` to position `to`, or last if `to` is past the end.
    /// Returns whether the order changed.
    pub fn move_tab(&mut self, id: SessionId, to: usize) -> bool {
        let from = self.tabs.iter().position(|&tab| tab == id);
        move_to(&mut self.tabs, from, Some(to))
    }

    /// Moves project `root` to where project `onto` is. Returns whether the
    /// order changed.
    pub fn move_project(&mut self, root: &Path, onto: &Path) -> bool {
        let at = |root: &Path| {
            self.projects
                .iter()
                .position(|project| project.root == root)
        };
        let (from, to) = (at(root), at(onto));
        move_to(&mut self.projects, from, to)
    }

    /// Moves session `id` to where session `onto` is, if both work in the
    /// same directory, which the sidebar lists together. Returns whether
    /// the order changed.
    pub fn move_session(&mut self, id: SessionId, onto: SessionId) -> bool {
        let (Some(moved), Some(target)) = (self.session(id), self.session(onto)) else {
            return false;
        };
        if moved.dir != target.dir {
            return false;
        }
        let at = |id: SessionId| self.sessions.iter().position(|session| session.id == id);
        let (from, to) = (at(id), at(onto));
        move_to(&mut self.sessions, from, to)
    }

    /// Shows `id` beside the active tab. Returns `false`, changing nothing,
    /// if `id` is the active session or does not exist.
    pub fn split(&mut self, id: SessionId, side: SplitSide) -> bool {
        if self.active == Some(id) || self.session(id).is_none() {
            return false;
        }
        self.split = Some(Split { session: id, side });
        true
    }

    /// Closes a tab; the session keeps running.
    pub fn close_tab(&mut self, id: SessionId) {
        self.drop_tabs(|tab| tab == id);
    }

    /// Removes one session.
    pub fn remove_session(&mut self, id: SessionId) {
        self.remove_sessions(|session| session.id == id);
    }

    /// Removes the sessions working in `dir`, returning them.
    pub fn remove_sessions_in(&mut self, dir: &Path) -> Vec<SessionId> {
        self.remove_sessions(|session| session.dir == dir)
    }

    fn remove_sessions(&mut self, matches: impl Fn(&SessionRecord) -> bool) -> Vec<SessionId> {
        let removed: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|session| matches(session))
            .map(|session| session.id)
            .collect();
        self.sessions
            .retain(|session| !removed.contains(&session.id));
        self.drop_tabs(|tab| removed.contains(&tab));
        if self
            .split
            .is_some_and(|split| removed.contains(&split.session))
        {
            self.split = None;
        }
        removed
    }

    /// Removes matching tabs. If the active one goes, the most recently
    /// shown tab left becomes active, or else the overview.
    fn drop_tabs(&mut self, gone: impl Fn(SessionId) -> bool) {
        self.tabs.retain(|&tab| !gone(tab));
        self.recent.retain(|&tab| !gone(tab));
        if self.active.is_some_and(&gone) {
            self.active = self.recent.first().copied();
        }
    }
}

/// Moves the item at `from` to position `to`, or last if `to` is past the
/// end. Returns whether the order changed.
fn move_to<T>(items: &mut Vec<T>, from: Option<usize>, to: Option<usize>) -> bool {
    let (Some(from), Some(to)) = (from, to) else {
        return false;
    };
    let to = to.min(items.len() - 1);
    if from == to {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    true
}

/// The session `steps` places away from `current` in `order`, wrapping
/// around; the first one if nothing is current.
pub fn cycle(order: &[SessionId], current: Option<SessionId>, steps: isize) -> Option<SessionId> {
    let len = order.len() as isize;
    if len == 0 {
        return None;
    }
    let index = match current.and_then(|id| order.iter().position(|&other| other == id)) {
        Some(index) => (index as isize + steps).rem_euclid(len),
        None => 0,
    };
    Some(order[index as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_with_sessions(count: usize) -> (Workspace, Vec<SessionId>) {
        let mut workspace = Workspace::default();
        workspace.add_project("/repo".into());
        let ids = (0..count)
            .map(|i| {
                workspace.add_session(
                    Path::new("/repo"),
                    format!("/tree{i}").into(),
                    "pi",
                    Vec::new(),
                )
            })
            .collect();
        (workspace, ids)
    }

    #[test]
    fn projects_are_unique() {
        let mut workspace = Workspace::default();
        assert!(workspace.add_project("/repo".into()));
        assert!(!workspace.add_project("/repo".into()));
        assert_eq!(workspace.projects[0].name(), "repo");
    }

    #[test]
    fn hiding_a_project_keeps_its_sessions() {
        let (mut workspace, ids) = workspace_with_sessions(2);
        let root = Path::new("/repo");
        assert!(workspace.set_project_hidden(root, true));
        assert!(!workspace.set_project_hidden(root, true));
        assert!(!workspace.set_project_hidden(Path::new("/missing"), true));
        assert!(workspace.projects[0].hidden);
        assert_eq!(workspace.tabs, ids);
        assert_eq!(workspace.sessions.len(), 2);

        let restored: Workspace =
            serde_json::from_str(&serde_json::to_string(&workspace).unwrap()).unwrap();
        assert!(restored.projects[0].hidden);

        assert!(workspace.set_project_hidden(root, false));
        assert!(!workspace.projects[0].hidden);
    }

    #[test]
    fn sessions_of_hidden_projects_are_not_shown() {
        let mut workspace = Workspace::default();
        workspace.add_project("/a".into());
        workspace.add_project("/b".into());
        let a = workspace.add_session(Path::new("/a"), "/a".into(), "pi", Vec::new());
        let b = workspace.add_session(Path::new("/b"), "/b".into(), "pi", Vec::new());
        let shown = |workspace: &Workspace| -> Vec<SessionId> {
            workspace
                .shown_sessions()
                .map(|session| session.id)
                .collect()
        };
        assert_eq!(shown(&workspace), [a, b]);
        workspace.set_project_hidden(Path::new("/a"), true);
        assert_eq!(shown(&workspace), [b]);
        assert_eq!(workspace.sessions.len(), 2, "hidden sessions keep running");
    }

    #[test]
    fn showing_all_projects_reveals_every_hidden_one() {
        let mut workspace = Workspace::default();
        for root in ["/a", "/b", "/c"] {
            workspace.add_project(root.into());
        }
        assert!(!workspace.show_all_projects());
        workspace.set_project_hidden(Path::new("/a"), true);
        workspace.set_project_hidden(Path::new("/c"), true);
        assert!(workspace.show_all_projects());
        assert!(workspace.projects.iter().all(|project| !project.hidden));
    }

    #[test]
    fn adding_a_hidden_project_shows_it_again() {
        let mut workspace = Workspace::default();
        workspace.add_project("/repo".into());
        workspace.set_project_hidden(Path::new("/repo"), true);
        assert!(workspace.add_project("/repo".into()));
        assert_eq!(workspace.projects.len(), 1);
        assert!(!workspace.projects[0].hidden);
        assert!(!workspace.add_project("/repo".into()));
    }

    #[test]
    fn projects_saved_before_hiding_existed_are_shown() {
        let workspace: Workspace =
            serde_json::from_str(r#"{"projects":[{"root":"/repo"}]}"#).unwrap();
        assert!(!workspace.projects[0].hidden);
        assert!(
            !serde_json::to_string(&workspace)
                .unwrap()
                .contains("hidden"),
            "shown projects save as before"
        );
    }

    #[test]
    fn new_sessions_get_fresh_ids_and_open_as_tabs() {
        let (workspace, ids) = workspace_with_sessions(3);
        assert_eq!(ids, [SessionId(1), SessionId(2), SessionId(3)]);
        assert_eq!(workspace.tabs, ids);
        assert_eq!(workspace.active, Some(SessionId(3)));
    }

    #[test]
    fn opening_adds_a_tab_only_once() {
        let (mut workspace, ids) = workspace_with_sessions(2);
        workspace.close_tab(ids[0]);
        workspace.open(ids[0]);
        workspace.open(ids[0]);
        assert_eq!(workspace.tabs, [ids[1], ids[0]]);
        assert_eq!(workspace.active, Some(ids[0]));
    }

    #[test]
    fn closing_the_active_tab_shows_the_one_shown_before() {
        let (mut workspace, ids) = workspace_with_sessions(4);
        workspace.open(ids[0]);
        workspace.open(ids[2]);
        workspace.close_tab(ids[2]);
        assert_eq!(workspace.active, Some(ids[0]), "not the neighbour");
        workspace.close_tab(ids[0]);
        assert_eq!(workspace.active, Some(ids[3]));
        workspace.close_tab(ids[3]);
        assert_eq!(workspace.active, Some(ids[1]));
        workspace.close_tab(ids[1]);
        assert_eq!(workspace.active, None);
        assert_eq!(
            workspace.sessions.len(),
            4,
            "closing a tab keeps the session"
        );
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        workspace.close_tab(ids[0]);
        assert_eq!(workspace.active, Some(ids[2]));
        assert_eq!(workspace.tabs, [ids[1], ids[2]]);
    }

    #[test]
    fn removing_a_worktree_or_project_removes_its_sessions_and_tabs() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        workspace.open(ids[0]);
        workspace.open(ids[2]);
        assert_eq!(workspace.remove_sessions_in(Path::new("/tree2")), [ids[2]]);
        assert_eq!(workspace.active, Some(ids[0]));
        assert_eq!(
            workspace.remove_project(Path::new("/repo")),
            [ids[0], ids[1]]
        );
        assert!(workspace.projects.is_empty());
        assert!(workspace.tabs.is_empty());
        assert_eq!(workspace.active, None);
    }

    #[test]
    fn an_agent_started_by_hand_reopens_after_restart() {
        let (mut workspace, ids) = workspace_with_sessions(1);
        let resume = vec!["resume".to_owned(), "abc".to_owned()];
        assert!(workspace.adopt_conversation(ids[0], "codex", resume.clone()));
        assert!(!workspace.adopt_conversation(ids[0], "codex", resume.clone()));
        assert!(!workspace.adopt_conversation(SessionId(99), "codex", resume.clone()));
        let restored: Workspace =
            serde_json::from_str(&serde_json::to_string(&workspace).unwrap()).unwrap();
        let session = restored.session(ids[0]).unwrap();
        assert_eq!(session.agent, "codex");
        assert_eq!(session.resume, resume);
    }

    #[test]
    fn entering_an_agent_by_hand_is_kept_before_any_conversation() {
        let (mut workspace, ids) = workspace_with_sessions(1);
        let resume = vec!["--session".to_owned(), "/pi.jsonl".to_owned()];
        workspace.adopt_conversation(ids[0], "pi", resume.clone());
        workspace.set_at_shell(ids[0], true);

        assert!(workspace.enter_agent(ids[0], "pi"));
        let session = workspace.session(ids[0]).unwrap();
        assert!(!session.at_shell);
        assert_eq!(
            session.resume, resume,
            "the same agent keeps its conversation"
        );
        assert!(!workspace.enter_agent(ids[0], "pi"));

        assert!(workspace.enter_agent(ids[0], "codex"));
        let session = workspace.session(ids[0]).unwrap();
        assert_eq!(session.agent, "codex");
        assert!(session.resume.is_empty(), "another agent starts afresh");
        assert!(!workspace.enter_agent(SessionId(99), "codex"));
    }

    #[test]
    fn returning_to_the_shell_survives_restarts() {
        let (mut workspace, ids) = workspace_with_sessions(1);
        assert!(workspace.set_at_shell(ids[0], true));
        assert!(!workspace.set_at_shell(ids[0], true));
        assert!(!workspace.set_at_shell(SessionId(99), true));
        let mut restored: Workspace =
            serde_json::from_str(&serde_json::to_string(&workspace).unwrap()).unwrap();
        assert!(restored.session(ids[0]).unwrap().at_shell);

        let resume = restored.session(ids[0]).unwrap().resume.clone();
        assert!(
            restored.adopt_conversation(ids[0], "pi", resume),
            "entering the same conversation again leaves the shell"
        );
        assert!(!restored.session(ids[0]).unwrap().at_shell);
    }

    #[test]
    fn recency_survives_restarts_and_moving_tabs() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        workspace.open(ids[0]);
        workspace.open(ids[1]);
        assert!(workspace.move_tab(ids[1], 0));
        assert_eq!(workspace.tabs, [ids[1], ids[0], ids[2]]);
        let mut restored: Workspace =
            serde_json::from_str(&serde_json::to_string(&workspace).unwrap()).unwrap();
        restored.repair();
        restored.close_tab(ids[1]);
        assert_eq!(restored.active, Some(ids[0]));
    }

    #[test]
    fn tabs_move_to_the_position_they_are_dropped_on() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        assert!(workspace.move_tab(ids[0], 2));
        assert_eq!(workspace.tabs, [ids[1], ids[2], ids[0]]);
        assert!(workspace.move_tab(ids[0], 0));
        assert_eq!(workspace.tabs, [ids[0], ids[1], ids[2]]);
        assert!(workspace.move_tab(ids[0], 99), "past the end moves last");
        assert_eq!(workspace.tabs, [ids[1], ids[2], ids[0]]);
        assert!(!workspace.move_tab(ids[0], 2));
        assert!(!workspace.move_tab(SessionId(99), 0));
        assert_eq!(
            workspace.active,
            Some(ids[2]),
            "moving does not switch tabs"
        );
    }

    #[test]
    fn projects_move_to_the_project_they_are_dropped_on() {
        let mut workspace = Workspace::default();
        for root in ["/a", "/b", "/c"] {
            workspace.add_project(root.into());
        }
        let order = |workspace: &Workspace| -> Vec<PathBuf> {
            workspace
                .projects
                .iter()
                .map(|project| project.root.clone())
                .collect()
        };
        assert!(workspace.move_project(Path::new("/a"), Path::new("/c")));
        assert_eq!(order(&workspace), ["/b", "/c", "/a"].map(PathBuf::from));
        assert!(workspace.move_project(Path::new("/a"), Path::new("/b")));
        assert_eq!(order(&workspace), ["/a", "/b", "/c"].map(PathBuf::from));
        assert!(!workspace.move_project(Path::new("/a"), Path::new("/a")));
        assert!(!workspace.move_project(Path::new("/missing"), Path::new("/a")));
        assert!(!workspace.move_project(Path::new("/a"), Path::new("/missing")));
    }

    #[test]
    fn sessions_move_only_among_those_in_the_same_directory() {
        let mut workspace = Workspace::default();
        workspace.add_project("/repo".into());
        let add = |workspace: &mut Workspace, dir: &str| {
            workspace.add_session(Path::new("/repo"), dir.into(), "pi", Vec::new())
        };
        let a = add(&mut workspace, "/repo");
        let b = add(&mut workspace, "/repo");
        let other = add(&mut workspace, "/tree");
        let order = |workspace: &Workspace| -> Vec<SessionId> {
            workspace
                .sessions
                .iter()
                .map(|session| session.id)
                .collect()
        };
        assert!(workspace.move_session(b, a));
        assert_eq!(order(&workspace), [b, a, other]);
        assert!(!workspace.move_session(other, a), "another worktree");
        assert!(!workspace.move_session(a, SessionId(99)));
        assert_eq!(order(&workspace), [b, a, other]);
    }

    #[test]
    fn state_without_recency_closes_to_the_earliest_tab() {
        let (workspace, ids) = workspace_with_sessions(3);
        let mut json = serde_json::to_value(&workspace).unwrap();
        json.as_object_mut().unwrap().remove("recent");
        let mut restored: Workspace = serde_json::from_value(json).unwrap();
        restored.repair();
        restored.close_tab(ids[2]);
        assert_eq!(restored.active, Some(ids[0]));
    }

    #[test]
    fn repair_fixes_tabs_from_older_state() {
        let (mut workspace, ids) = workspace_with_sessions(2);
        workspace.tabs = vec![SessionId(99)];
        workspace.active = Some(ids[0]);
        workspace.repair();
        assert_eq!(workspace.tabs, [ids[0]]);

        workspace.active = Some(SessionId(99));
        workspace.repair();
        assert_eq!(workspace.active, Some(ids[0]));
    }

    #[test]
    fn splits_show_another_session() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        assert!(
            !workspace.split(ids[2], SplitSide::Right),
            "the active session cannot split"
        );
        assert!(!workspace.split(SessionId(99), SplitSide::Right));
        assert!(workspace.split(ids[0], SplitSide::Below));
        assert_eq!(
            workspace.split,
            Some(Split {
                session: ids[0],
                side: SplitSide::Below
            })
        );

        workspace.open(ids[1]);
        assert!(workspace.split.is_some(), "other tabs keep the split");
        workspace.open(ids[0]);
        assert_eq!(
            workspace.split, None,
            "opening the split session ends the split"
        );
    }

    #[test]
    fn removing_the_split_session_ends_the_split() {
        let (mut workspace, ids) = workspace_with_sessions(2);
        workspace.split(ids[0], SplitSide::Right);
        workspace.remove_session(ids[0]);
        assert_eq!(workspace.split, None);
    }

    #[test]
    fn repair_drops_a_split_of_the_active_session() {
        let (mut workspace, ids) = workspace_with_sessions(2);
        workspace.split = Some(Split {
            session: ids[1],
            side: SplitSide::Right,
        });
        workspace.repair();
        assert_eq!(workspace.split, None);
    }

    #[test]
    fn cycling_wraps_around() {
        let order = [SessionId(1), SessionId(2), SessionId(3)];
        assert_eq!(cycle(&order, Some(SessionId(3)), 1), Some(SessionId(1)));
        assert_eq!(cycle(&order, Some(SessionId(1)), -1), Some(SessionId(3)));
        assert_eq!(cycle(&order, None, 1), Some(SessionId(1)));
        assert_eq!(cycle(&[], None, 1), None);
    }

    #[test]
    fn ids_round_trip_through_text() {
        assert_eq!(SessionId(42).to_string().parse(), Ok(SessionId(42)));
        assert!("x".parse::<SessionId>().is_err());
    }

    #[test]
    fn round_trips_through_json() {
        let (workspace, _) = workspace_with_sessions(2);
        let json = serde_json::to_string(&workspace).unwrap();
        assert_eq!(serde_json::from_str::<Workspace>(&json).unwrap(), workspace);
    }

    #[test]
    fn names_survive_restart_and_blank_names_restore_the_default() {
        let (mut workspace, ids) = workspace_with_sessions(2);
        assert!(workspace.rename_session(ids[0], "  Review PR 385  "));
        assert_eq!(
            workspace.session(ids[0]).unwrap().label.as_deref(),
            Some("Review PR 385")
        );
        let saved = serde_json::to_string(&workspace).unwrap();
        let restored: Workspace = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored.session(ids[0]).unwrap().label.as_deref(),
            Some("Review PR 385")
        );
        assert_eq!(restored.session(ids[1]).unwrap().label, None);

        assert!(workspace.rename_session(ids[0], "  "));
        assert_eq!(workspace.session(ids[0]).unwrap().label, None);
        assert!(!workspace.rename_session(SessionId(999), "missing"));
    }

    #[test]
    fn automatic_names_never_replace_a_manual_name() {
        let (mut workspace, ids) = workspace_with_sessions(1);
        assert!(!workspace.auto_name_session(ids[0], "/help"));
        assert!(workspace.auto_name_session(ids[0], "Review PR 385"));
        assert!(!workspace.auto_name_session(ids[0], "Another task"));
        assert_eq!(
            workspace.session(ids[0]).unwrap().label.as_deref(),
            Some("Review PR 385")
        );
        workspace.rename_session(ids[0], "My task");
        assert!(!workspace.auto_name_session(ids[0], "Agent prompt"));
        assert_eq!(
            workspace.session(ids[0]).unwrap().label.as_deref(),
            Some("My task")
        );
    }

    #[test]
    fn the_active_tab_and_its_split_are_on_screen() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        assert!(workspace.split(ids[0], SplitSide::Right));
        assert_eq!(workspace.on_screen().collect::<Vec<_>>(), [ids[2], ids[0]]);
        workspace.active = None;
        assert_eq!(workspace.on_screen().count(), 0);
    }

    #[test]
    fn choosing_the_agent_name_keeps_it_after_restart() {
        let (mut workspace, ids) = workspace_with_sessions(1);
        assert!(workspace.auto_name_session(ids[0], "Review PR 385"));
        workspace.rename_session(ids[0], "");
        let saved = serde_json::to_string(&workspace).unwrap();
        let mut restored: Workspace = serde_json::from_str(&saved).unwrap();
        assert!(!restored.auto_name_session(ids[0], "Another task"));
        assert_eq!(restored.session(ids[0]).unwrap().label, None);
    }

    #[test]
    fn conversations_to_resume_survive_restarts() {
        let (mut workspace, ids) = workspace_with_sessions(1);
        let resume = vec!["--resume".to_owned(), "abc".to_owned()];
        assert!(workspace.set_resume(ids[0], resume.clone()));
        assert!(!workspace.set_resume(ids[0], resume.clone()));
        assert!(!workspace.set_resume(SessionId(999), resume.clone()));
        let restored: Workspace =
            serde_json::from_str(&serde_json::to_string(&workspace).unwrap()).unwrap();
        assert_eq!(restored.session(ids[0]).unwrap().resume, resume);
    }
}
