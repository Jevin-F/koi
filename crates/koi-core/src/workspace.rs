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

/// A git repository added by the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub root: PathBuf,
}

impl Project {
    /// Display name: the repository's directory name.
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
    /// The tab on screen; `None` shows the overview.
    pub active: Option<SessionId>,
    /// Never the active session, which is shown once only.
    pub split: Option<Split>,
}

impl Workspace {
    /// Adds a project; returns `false` if it is already present.
    pub fn add_project(&mut self, root: PathBuf) -> bool {
        if self.projects.iter().any(|project| project.root == root) {
            return false;
        }
        self.projects.push(Project { root });
        true
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
        let name = name.trim().chars().take(80).collect::<String>();
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
        self.active = Some(id);
        if self.split.is_some_and(|split| split.session == id) {
            self.split = None;
        }
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

    /// Removes matching tabs. If the active one goes, the tab after it
    /// becomes active, or else the one before, or else the overview.
    fn drop_tabs(&mut self, gone: impl Fn(SessionId) -> bool) {
        if let Some(index) = self
            .active
            .and_then(|id| self.tabs.iter().position(|&tab| tab == id))
            && gone(self.tabs[index])
        {
            let (before, after) = self.tabs.split_at(index);
            self.active = after
                .iter()
                .chain(before.iter().rev())
                .copied()
                .find(|&tab| !gone(tab));
        }
        self.tabs.retain(|&tab| !gone(tab));
    }
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
    fn closing_the_active_tab_activates_a_neighbour() {
        let (mut workspace, ids) = workspace_with_sessions(3);
        workspace.active = Some(ids[1]);
        workspace.close_tab(ids[1]);
        assert_eq!(workspace.active, Some(ids[2]));
        workspace.close_tab(ids[2]);
        assert_eq!(workspace.active, Some(ids[0]));
        workspace.close_tab(ids[0]);
        assert_eq!(workspace.active, None);
        assert_eq!(
            workspace.sessions.len(),
            3,
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
        assert_eq!(workspace.remove_sessions_in(Path::new("/tree2")), [ids[2]]);
        assert_eq!(workspace.active, Some(ids[1]));
        assert_eq!(
            workspace.remove_project(Path::new("/repo")),
            [ids[0], ids[1]]
        );
        assert!(workspace.projects.is_empty());
        assert!(workspace.tabs.is_empty());
        assert_eq!(workspace.active, None);
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
