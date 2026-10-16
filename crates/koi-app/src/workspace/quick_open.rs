//! Quick open: search sessions and the commands that start or resume agents.

use std::path::PathBuf;

use gpui_kit::*;
use koi_core::SessionId;
use koi_core::project_files;
use rust_i18n::t;

use super::{QuickOpen, WorkspaceView, directory_name};
use crate::picker::{Picker, PickerItem};

/// What choosing an entry does.
#[derive(Clone)]
enum Command {
    Open(SessionId),
    Start {
        project: PathBuf,
        dir: PathBuf,
        agent: String,
    },
    History {
        project: PathBuf,
        dir: PathBuf,
    },
    OpenFile {
        session: SessionId,
        path: String,
    },
}

impl WorkspaceView {
    /// Sessions first, then commands for every worktree, then the files of
    /// the active session's worktree.
    fn quick_open_items(&self) -> Vec<PickerItem<Command>> {
        let mut items: Vec<PickerItem<Command>> = self
            .workspace
            .sessions
            .iter()
            .map(|record| PickerItem {
                kind: t!("quick_open.session").into(),
                text: format!(
                    "{} · {} · {}",
                    self.session_name(record),
                    self.place_name(record),
                    directory_name(&record.project)
                ),
                value: Command::Open(record.id),
            })
            .collect();
        for project in &self.workspace.projects {
            let Some(Ok(trees)) = self.worktrees.get(&project.root) else {
                continue;
            };
            for tree in trees {
                let place = self.place_of(&project.root, &tree.path);
                let project_name = project.name();
                items.extend(self.agents.iter().map(|agent| PickerItem {
                    kind: t!("quick_open.start").into(),
                    text: format!(
                        "{} · {place} · {project_name}",
                        t!("quick_open.start_agent", agent = agent.name)
                    ),
                    value: Command::Start {
                        project: project.root.clone(),
                        dir: tree.path.clone(),
                        agent: agent.id.clone(),
                    },
                }));
                items.push(PickerItem {
                    kind: t!("quick_open.history").into(),
                    text: format!("{} · {place} · {project_name}", t!("action.history")),
                    value: Command::History {
                        project: project.root.clone(),
                        dir: tree.path.clone(),
                    },
                });
            }
        }
        if let Some(record) = self
            .workspace
            .active
            .and_then(|id| self.workspace.session(id))
        {
            // Without a readable file list, quick open still offers everything else.
            let files = project_files::files(&record.dir).unwrap_or_default();
            items.extend(files.into_iter().map(|path| PickerItem {
                kind: t!("quick_open.file").into(),
                text: path.clone(),
                value: Command::OpenFile {
                    session: record.id,
                    path,
                },
            }));
        }
        items
    }

    pub(super) fn open_quick_open(
        &mut self,
        _: &QuickOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = self.quick_open_items();
        let workspace = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            Picker::new(
                items,
                t!("quick_open.placeholder").into(),
                move |command, window, cx| {
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.run_command(command, window, cx)
                        })
                        .ok();
                },
                window,
                cx,
            )
        });
        Picker::open(picker, None, window, cx);
    }

    fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::Open(id) => self.open(id, window, cx),
            Command::Start {
                project,
                dir,
                agent,
            } => self.start_agent(&project, dir, &agent, Vec::new(), window, cx),
            Command::History { project, dir } => self.open_history(project, dir, window, cx),
            Command::OpenFile { session, path } => self.open_file(session, path, window, cx),
        }
    }
}
