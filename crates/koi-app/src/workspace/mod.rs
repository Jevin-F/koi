//! The workspace view: projects, their worktrees and the agent sessions
//! running in them. The list sits on the left; on the right, tabs switch
//! between the overview and the sessions opened so far.

mod activity;
mod changes;
mod changes_view;
mod detached;
mod dialogs;
mod file_icon;
mod files_view;
mod history;
mod layout;
mod opencode;
mod overview;
mod quick_open;
mod refresh;
mod replies;
mod resources;
mod resume;
mod sidebar;
mod split;
mod status;
mod tabs;
mod unseen;
mod views;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Icon, WindowExt, v_flex};
use gpui_kit::*;
use koi_core::git::{self, Branch, FileChange, Worktree};
use koi_core::history::Replies;
use koi_core::integration::{
    EXECUTABLE_ENV, Integration, PI_TERMINAL_ENV, RESUME_ENV, STATUS_ENV, TITLE_ENV,
};
use koi_core::resources::Usage;
use koi_core::{
    Activity, Agent, AppPaths, Config, SessionId, SessionRecord, Workspace, cycle, store,
};
use koi_term::{CommandBuilder, Signal};
use rust_i18n::t;

use crate::APP_NAME;
use crate::fs_watcher::FsWatcher;
use crate::terminal::{TerminalEvent, TerminalView};
pub use files_view::{CONTEXT as FILES_CONTEXT, Save};
use status::Status;
use views::Panel;

pub(crate) const CONTEXT: &str = "Workspace";

actions!(
    workspace,
    [
        AddProject,
        QuickOpen,
        ShowOverview,
        NextTab,
        PreviousTab,
        CloseTab,
        ToggleChanges,
        ToggleFiles
    ]
);

/// Activates the tab at this position, counting from zero.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = workspace, no_json)]
pub struct SelectTab(pub usize);

/// Binds the tab number shortcuts; the other workspace shortcuts can be
/// changed and are bound by [`crate::shortcuts`].
pub fn init(cx: &mut App) {
    cx.bind_keys(
        (1..=9).map(|n| KeyBinding::new(&format!("cmd-{n}"), SelectTab(n - 1), Some(CONTEXT))),
    );
}

/// A session whose program has been started.
struct Running {
    view: Entity<TerminalView>,
    /// The terminal is at an interactive shell: a plain shell session, or
    /// one whose agent has exited.
    shell: bool,
    /// Agent started by hand after this terminal returned to its shell.
    reentered: Option<String>,
    /// Title set by the program, if any.
    title: Option<String>,
    integration: Option<Integration>,
    /// Latest activity; `None` until it is known.
    activity: Option<Activity>,
    /// The agent reports no activity, so it is told from the terminal output.
    watches_output: bool,
    /// The prompt last submitted and when, until the agent starts on it.
    prompt: Option<(String, Instant)>,
    /// Finished a turn or asked something while the user was not looking.
    unseen: bool,
    _events: Subscription,
}

pub struct WorkspaceView {
    paths: AppPaths,
    agents: Vec<Agent>,
    shell: String,
    workspace: Workspace,
    /// Worktrees of each project, or why they could not be listed. A plain
    /// folder is listed as its only worktree.
    worktrees: HashMap<PathBuf, Result<Vec<Worktree>, String>>,
    /// Projects that are plain folders rather than git repositories.
    folders: HashSet<PathBuf>,
    running: HashMap<SessionId, Running>,
    focus: FocusHandle,
    window_title: String,
    sidebar_resizing: bool,
    sidebar_drag: Option<(Pixels, Pixels)>,
    /// `None` if status files cannot be watched; sessions then show no activity.
    status_watcher: Option<FsWatcher>,
    /// Watches git directories; `None` if unavailable, and lists then
    /// refresh only after Koi's own changes.
    git_watcher: Option<FsWatcher>,
    /// Shared git directory of each project.
    git_dirs: HashMap<PathBuf, PathBuf>,
    /// Watches the files of every worktree; `None` if unavailable, and change
    /// counts then update on git events only.
    tree_watcher: Option<FsWatcher>,
    /// Uncommitted changes of each worktree, or why they could not be read.
    changes: HashMap<PathBuf, Result<Vec<FileChange>, String>>,
    /// Worktrees waiting to be recounted.
    stale_trees: HashSet<PathBuf>,
    changes_refresh: Option<Task<()>>,
    /// Changes panels by worktree, created when first shown.
    changes_views: HashMap<PathBuf, Entity<changes_view::ChangesView>>,
    /// Files panels by worktree, created when first shown.
    files_views: HashMap<PathBuf, Entity<files_view::FilesView>>,
    /// Sessions showing a panel instead of their terminal.
    panels: HashMap<SessionId, Panel>,
    /// Sessions moved to windows of their own.
    detached: HashMap<SessionId, AnyWindowHandle>,
    /// Latest memory and CPU of each running session.
    usage: HashMap<SessionId, Usage>,
    /// Latest memory of Koi itself.
    app_memory: Option<u64>,
    /// Opening line of the latest reply of each agent session, once saved.
    replies: HashMap<SessionId, String>,
    /// Replies already read, kept between refreshes.
    reply_cache: Replies,
    /// Samples resources while the window is active.
    sampler: Option<Task<()>>,
    /// Number of unseen sessions last shown on the Dock icon.
    badge: usize,
    _watch_tasks: Vec<Task<()>>,
}

impl WorkspaceView {
    /// Loads configuration and the saved workspace, restarting saved
    /// sessions. `problems` found earlier during startup are shown with the
    /// ones found here.
    pub fn new(
        paths: AppPaths,
        mut problems: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = Config::load(&paths.config_file()).unwrap_or_else(|error| {
            problems.push(t!("error.config", error = error).into_owned());
            Config::default()
        });
        let mut workspace = load_workspace(&paths.state_file(), &mut problems);
        let agents = config.agents();
        let recovered_resume = resume::recover_resumes(&mut workspace, &agents, &paths);
        let status_watcher = activity::watch_status_files(&paths)
            .map_err(|error| problems.push(t!("error.status", error = error).into_owned()))
            .ok();
        let git_watcher = FsWatcher::new()
            .map_err(|error| problems.push(t!("error.watch", error = error).into_owned()))
            .ok();
        let tree_watcher = FsWatcher::new()
            .map_err(|error| problems.push(t!("error.watch", error = error).into_owned()))
            .ok();
        let watch_tasks = [
            status_watcher.as_ref().map(|watcher| {
                Self::on_changes(watcher.signal(), Self::read_status_files, window, cx)
            }),
            git_watcher.as_ref().map(|watcher| {
                Self::on_changes(watcher.signal(), Self::refresh_changed_projects, window, cx)
            }),
            tree_watcher.as_ref().map(|watcher| {
                Self::on_changes(watcher.signal(), Self::refresh_changed_trees, window, cx)
            }),
            Some(Self::poll_opencode(window, cx)),
            Some(Self::watch_output(window, cx)),
        ];

        let mut this = Self {
            paths,
            agents,
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned()),
            workspace,
            worktrees: HashMap::new(),
            folders: HashSet::new(),
            running: HashMap::new(),
            focus: cx.focus_handle(),
            window_title: String::new(),
            sidebar_resizing: false,
            sidebar_drag: None,
            status_watcher,
            git_watcher,
            git_dirs: HashMap::new(),
            tree_watcher,
            changes: HashMap::new(),
            stale_trees: HashSet::new(),
            changes_refresh: None,
            changes_views: HashMap::new(),
            files_views: HashMap::new(),
            panels: HashMap::new(),
            detached: HashMap::new(),
            usage: HashMap::new(),
            app_memory: None,
            replies: HashMap::new(),
            reply_cache: Replies::default(),
            sampler: None,
            badge: 0,
            _watch_tasks: watch_tasks.into_iter().flatten().collect(),
        };
        if recovered_resume {
            this.persist(window, cx);
        }
        for project in this.workspace.projects.clone() {
            this.refresh_worktrees(&project.root, cx);
        }
        for record in this.workspace.sessions.clone() {
            if let Err(problem) = this.start(&record, window, cx) {
                problems.push(problem);
                this.workspace.remove_session(record.id);
            }
        }
        activity::handle_notification_clicks(window, cx);
        cx.observe_window_activation(window, |this, window, cx| {
            this.track_resources(window.is_window_active(), cx);
            this.mark_seen(window, cx);
        })
        .detach();
        this.track_resources(window.is_window_active(), cx);
        this.focus_active(window, cx);
        window.defer(cx, move |window, cx| {
            for problem in problems {
                window.push_notification(Notification::error(problem), cx);
            }
        });
        this
    }

    fn report(&self, problem: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(problem), cx);
    }

    fn persist(&self, window: &mut Window, cx: &mut App) {
        if let Err(error) = store::save(&self.paths.state_file(), &self.workspace) {
            self.report(t!("error.save", error = error), window, cx);
        }
    }

    /// Runs `on_change` whenever `signal` is raised, for as long as the view lives.
    fn on_changes(
        signal: Signal,
        on_change: fn(&mut Self, &mut Window, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            while signal.wait().await {
                if this.update_in(cx, on_change).is_err() {
                    break;
                }
            }
        })
    }

    fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.id == id)
    }

    /// Starts the program of `record` in a new terminal, wired to report its
    /// activity when the agent supports it.
    fn start(
        &mut self,
        record: &SessionRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let agent = self
            .agent(&record.agent)
            .ok_or_else(|| t!("error.unknown_agent", agent = record.agent).into_owned())?;
        let integration = Integration::for_agent(agent)
            .filter(|integration| integration.reports_activity() && self.status_watcher.is_some());
        let extra: Vec<String> = record
            .resume
            .iter()
            .cloned()
            .chain(
                integration
                    .into_iter()
                    .flat_map(|integration| integration.args(&self.paths)),
            )
            .collect();
        let argv = agent.launch_command(&self.shell, &extra);
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        command.cwd(&record.dir);
        for (key, value) in PI_TERMINAL_ENV {
            command.env(key, value);
        }
        if self.status_watcher.is_some() && !agent.command.is_empty() {
            let status_file = self.paths.status_file(record.id);
            // A file left by an earlier run would report a stale state.
            let _ = std::fs::remove_file(&status_file);
            command.env(STATUS_ENV, status_file);
        }
        if matches!(integration, Some(Integration::Pi | Integration::Claude)) {
            let title_file = self.paths.title_file(record.id);
            let _ = std::fs::remove_file(&title_file);
            command.env(TITLE_ENV, title_file);
        }
        if integration.is_some() {
            let resume_file = self.paths.resume_file(record.id);
            let _ = std::fs::remove_file(&resume_file);
            command.env(RESUME_ENV, resume_file);
        }
        if matches!(integration, Some(Integration::Claude | Integration::Codex))
            && let Ok(executable) = std::env::current_exe()
        {
            command.env(EXECUTABLE_ENV, executable);
        }
        // Pi and Claude Code report their activity; Codex only finished turns.
        let watches_output = !agent.command.is_empty()
            && !matches!(integration, Some(Integration::Pi | Integration::Claude));
        let view = TerminalView::spawn(command, cx)
            .map_err(|error| t!("error.start", agent = agent.name, error = error).into_owned())?;

        let id = record.id;
        let events = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
            match event {
                TerminalEvent::TitleChanged(title) => {
                    if let Some(running) = this.running.get_mut(&id) {
                        running.title = title.clone();
                    }
                    this.follow_titled_session(id, window, cx);
                }
                TerminalEvent::Submitted(prompt) => this.on_submitted(id, prompt.as_deref()),
                TerminalEvent::ProgramStatus(status) => {
                    this.on_program_status(id, status, window, cx)
                }
                TerminalEvent::Exited => this.on_exited(id, window, cx),
                TerminalEvent::RestartRequested => this.restart(id, window, cx),
            }
            cx.notify();
        });
        self.running.insert(
            id,
            Running {
                view,
                // A plain shell session starts at the shell.
                shell: agent.command.is_empty(),
                reentered: None,
                title: None,
                integration,
                activity: integration.map(|_| Activity::Idle),
                watches_output,
                prompt: None,
                unseen: false,
                _events: events,
            },
        );
        Ok(())
    }

    fn restart(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.workspace.session(id).cloned() else {
            return;
        };
        self.running.remove(&id);
        self.update_badge();
        if let Err(problem) = self.start(&record, window, cx) {
            self.report(problem, window, cx);
        }
        self.focus_active(window, cx);
    }

    fn status(&self, id: SessionId, cx: &App) -> Status {
        let Some(running) = self.running.get(&id) else {
            return Status::Exited;
        };
        match running.view.read(cx).exit_code() {
            Some(Some(code)) if code != 0 => return Status::Failed,
            Some(_) => return Status::Exited,
            None => {}
        }
        if running.shell {
            return Status::Shell;
        }
        match running.activity {
            Some(Activity::Working) => Status::Working,
            Some(Activity::Waiting) => Status::Waiting,
            Some(Activity::Idle) if running.unseen => Status::Finished,
            Some(Activity::Idle) => Status::Idle,
            None => Status::Running,
        }
    }

    /// Focuses the active terminal, or the workspace itself on the overview
    /// and on a changes view.
    fn focus_active(&self, window: &mut Window, cx: &mut App) {
        let terminal = self
            .workspace
            .active
            .filter(|id| !self.panels.contains_key(id))
            .and_then(|id| self.running.get(&id));
        let focus = match terminal {
            Some(running) => running.view.read(cx).focus_handle().clone(),
            None => self.focus.clone(),
        };
        window.focus(&focus, cx);
    }

    /// Shows a session, opening a tab for it if needed.
    fn open(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.open(id);
        self.after_tab_change(window, cx);
    }

    fn close_tab(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.close_tab(id);
        self.after_tab_change(window, cx);
    }

    fn after_tab_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prune_panels();
        self.prune_detached(cx);
        self.mark_seen(window, cx);
        self.focus_active(window, cx);
        self.persist(window, cx);
        cx.notify();
    }

    fn start_agent(
        &mut self,
        project: &Path,
        dir: PathBuf,
        agent: &str,
        resume: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.workspace.add_session(project, dir, agent, resume);
        let record = self
            .workspace
            .session(id)
            .cloned()
            .expect("session was just added");
        if let Err(problem) = self.start(&record, window, cx) {
            self.workspace.remove_session(id);
            self.report(problem, window, cx);
        }
        self.after_tab_change(window, cx);
    }

    fn close_session(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.remove_session(id);
        self.running.remove(&id);
        self.update_badge();
        self.after_tab_change(window, cx);
    }

    fn add_project(&mut self, _: &AddProject, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("workspace.pick_folder").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.add_project_at(&path, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn add_project_at(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        // A folder inside a repository adds the whole repository.
        let root = git::repo_root(path).unwrap_or_else(|_| path.to_owned());
        if !self.workspace.add_project(root.clone()) {
            return self.report(t!("error.already_added", path = root.display()), window, cx);
        }
        self.refresh_worktrees(&root, cx);
        self.persist(window, cx);
        cx.notify();
    }

    fn remove_project(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        for id in self.workspace.remove_project(root) {
            self.running.remove(&id);
        }
        self.worktrees.remove(root);
        self.folders.remove(root);
        self.forget_git_dir(root);
        self.after_tab_change(window, cx);
    }

    /// Creates a worktree for `branch` in the background.
    fn create_worktree(
        &mut self,
        project: PathBuf,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match git::is_valid_branch_name(&project, &branch.name) {
            Ok(true) => {}
            Ok(false) => {
                let message = t!("error.invalid_branch", branch = branch.name);
                return self.report(message, window, cx);
            }
            Err(error) => return self.report(t!("error.git", error = error), window, cx),
        }
        let dir = self.paths.worktree_dir(&project, &branch.name);
        let task = cx.background_spawn({
            let project = project.clone();
            async move { git::add_worktree(&project, &dir, &branch.name, branch.remote.as_deref()) }
        });
        self.finish_git_task(project, task, window, cx);
    }

    /// Closes the worktree's sessions and removes it in the background.
    fn delete_worktree(
        &mut self,
        project: PathBuf,
        dir: PathBuf,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for id in self.workspace.remove_sessions_in(&dir) {
            self.running.remove(&id);
        }
        self.after_tab_change(window, cx);
        let task = cx.background_spawn({
            let project = project.clone();
            async move { git::remove_worktree(&project, &dir, force) }
        });
        self.finish_git_task(project, task, window, cx);
    }

    /// Reports the outcome of a background git task and refreshes the
    /// project's worktrees.
    fn finish_git_task(
        &mut self,
        project: PathBuf,
        task: Task<git::Result<()>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                if let Err(error) = result {
                    this.report(t!("error.git", error = error), window, cx);
                }
                this.refresh_worktrees(&project, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Closes the active session tab; on the overview, which has no close
    /// button, closes the window instead.
    fn close_active_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        match self.workspace.active {
            Some(id) => self.close_tab(id, window, cx),
            None => crate::main_window::close(cx),
        }
    }

    fn show_overview(&mut self, _: &ShowOverview, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.active = None;
        self.after_tab_change(window, cx);
    }

    fn step_tab(&mut self, steps: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = cycle(&self.workspace.tabs, self.workspace.active, steps) {
            self.open(id, window, cx);
        }
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.step_tab(1, window, cx);
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.step_tab(-1, window, cx);
    }

    fn select_tab(&mut self, action: &SelectTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&id) = self.workspace.tabs.get(action.0) {
            self.open(id, window, cx);
        }
    }

    /// Name of the agent running in a session: its own, one started by hand
    /// after it returned to the shell, or the agent id if the agent is gone.
    fn agent_name(&self, record: &SessionRecord) -> String {
        let running = self.running.get(&record.id);
        if running.is_some_and(|running| running.shell) {
            return "Shell".to_owned();
        }
        let id = running
            .and_then(|running| running.reentered.as_deref())
            .unwrap_or(&record.agent);
        self.agent(id)
            .map_or_else(|| id.to_owned(), |agent| agent.name.clone())
    }

    /// User task name, or the agent name until the session is named.
    fn session_name(&self, record: &SessionRecord) -> String {
        record
            .label
            .clone()
            .unwrap_or_else(|| self.agent_name(record))
    }

    /// Title for the window: the active session's agent and branch.
    fn title(&self) -> String {
        match self
            .workspace
            .active
            .and_then(|id| self.workspace.session(id))
        {
            Some(record) => format!(
                "{} — {}",
                self.session_name(record),
                self.place_name(record)
            ),
            None => APP_NAME.to_owned(),
        }
    }

    /// Whether the project is a plain folder, without branches or changes.
    fn is_folder(&self, project: &Path) -> bool {
        self.folders.contains(project)
    }

    /// The branch a session works on, or its directory name.
    fn place_name(&self, record: &SessionRecord) -> String {
        self.place_of(&record.project, &record.dir)
    }

    /// The branch checked out in `dir`, or the directory's name.
    fn place_of(&self, project: &Path, dir: &Path) -> String {
        self.worktrees
            .get(project)
            .and_then(|listed| listed.as_ref().ok())
            .and_then(|trees| trees.iter().find(|tree| tree.path == dir))
            .and_then(|tree| tree.branch.clone())
            .unwrap_or_else(|| directory_name(dir))
    }

    fn render_main(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.workspace.projects.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(gpui_kit::assets::IconName::FolderPlus).size_8())
                .child(t!("workspace.no_projects"))
                .child(
                    Button::new("add-project")
                        .primary()
                        .label(t!("action.add_project"))
                        .cursor_pointer()
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(AddProject), cx)),
                )
                .into_any_element();
        }
        let terminal = self.workspace.active.and_then(|id| self.running.get(&id));
        let content = match (self.render_active_panel(), self.workspace.active, terminal) {
            (Some(panel), _, _) => panel,
            (None, Some(id), _) if self.detached.contains_key(&id) => {
                self.render_detached_notice(id, cx)
            }
            (None, _, Some(running)) => running.view.clone().into_any_element(),
            (None, _, None) => self.render_overview(cx),
        };
        let content = self.with_split(content, cx);
        v_flex()
            .size_full()
            .child(self.render_tab_bar(cx))
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }
}

/// The last component of `path`, or the whole path if it has none.
fn directory_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Loads the saved workspace. An unreadable file is set aside so that saving
/// later does not overwrite it.
fn load_workspace(path: &Path, problems: &mut Vec<String>) -> Workspace {
    match store::load::<Workspace>(path) {
        Ok(mut workspace) => {
            workspace.repair();
            workspace
        }
        Err(error) => {
            let aside = path.with_extension("json.unreadable");
            let _ = std::fs::rename(path, &aside);
            problems.push(t!("error.restore", error = error, path = aside.display()).into_owned());
            Workspace::default()
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title();
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::add_project))
            .on_action(cx.listener(Self::open_quick_open))
            .on_action(cx.listener(Self::show_overview))
            .on_action(cx.listener(Self::toggle_changes))
            .on_action(cx.listener(Self::toggle_files))
            .on_action(cx.listener(Self::close_active_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::select_tab))
            .bg(cx.theme().background)
            .child(div().flex_1().min_h_0().child(self.render_body(window, cx)))
            .child(self.render_status_bar(cx))
    }
}
