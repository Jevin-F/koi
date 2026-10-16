//! Dialogs for creating and deleting worktrees and removing projects.

use std::path::PathBuf;

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;
use koi_core::SessionId;
use koi_core::git::{self, Branch};
use koi_core::integration::Integration;
use rust_i18n::t;

use super::{WorkspaceView, directory_name};
use crate::picker::{Picker, PickerItem};

impl WorkspaceView {
    /// Prompts for a task name to distinguish sessions on the same worktree.
    /// Enter saves the typed name; the option below goes back to the agent
    /// name, as does saving an empty name.
    pub(super) fn open_rename_session_dialog(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.workspace.session(id) else {
            return;
        };
        let use_agent_name = t!("dialog.use_agent_name", agent = self.agent_name(record));
        let current = record.label.clone().unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("dialog.session_name_placeholder"))
                .default_value(current)
        });
        let view = cx.entity().downgrade();
        window.open_dialog(cx, {
            let input = input.clone();
            move |dialog, _, cx| {
                let (input, view) = (input.clone(), view.clone());
                let reset = {
                    let view = view.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        window.close_dialog(cx);
                        view.update(cx, |this, cx| this.rename_session(id, "", window, cx))
                            .ok();
                    }
                };
                dialog
                    .title(t!("dialog.rename_session"))
                    .close_button(false)
                    .child(
                        v_flex()
                            .gap_2()
                            .child(Input::new(&input).id("session-name-input").cleanable(true))
                            .child(
                                div()
                                    .id("use-agent-name")
                                    .test_support()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .hover(|row| row.bg(cx.theme().list_hover))
                                    .on_click(reset)
                                    .child(use_agent_name.clone()),
                            ),
                    )
                    .on_ok(move |_, window, cx| {
                        let name = input.read(cx).value();
                        view.update(cx, |this, cx| this.rename_session(id, &name, window, cx))
                            .ok();
                        true
                    })
            }
        });
        // Opening a dialog focuses the dialog itself, so focus moves after.
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn rename_session(
        &mut self,
        id: SessionId,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.rename_session(id, name) {
            self.persist(window, cx);
            cx.notify();
        }
    }

    /// Lets the user pick a branch for a new worktree of `project`, or name
    /// a new one. Branches already checked out in a worktree are left out,
    /// as git allows each branch in only one worktree.
    pub(super) fn open_new_worktree_dialog(
        &mut self,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let branches = match git::branches(&project) {
            Ok(branches) => branches,
            Err(error) => return self.report(t!("error.git", error = error), window, cx),
        };
        let checked_out: Vec<String> = self
            .worktrees
            .get(&project)
            .and_then(|listed| listed.as_ref().ok())
            .into_iter()
            .flatten()
            .filter_map(|tree| tree.branch.clone())
            .collect();
        let known: Vec<String> = branches.iter().map(|branch| branch.name.clone()).collect();
        let items = branches
            .into_iter()
            .filter(|branch| !checked_out.contains(&branch.name))
            .map(|branch| PickerItem {
                kind: match branch.remote {
                    Some(_) => t!("branch.remote"),
                    None => t!("branch.local"),
                }
                .into(),
                text: branch.remote.clone().unwrap_or_else(|| branch.name.clone()),
                value: branch,
            })
            .collect();
        let workspace = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            Picker::new(
                items,
                t!("dialog.branch_placeholder").into(),
                move |branch, window, cx| {
                    workspace
                        .update(cx, |this, cx| {
                            this.create_worktree(project.clone(), branch, window, cx)
                        })
                        .ok();
                },
                window,
                cx,
            )
            .with_query_item(move |query| {
                let name = query.trim();
                (!name.is_empty() && !known.iter().any(|known| known == name)).then(|| PickerItem {
                    kind: t!("branch.new").into(),
                    text: t!("dialog.create_branch", name = name).into_owned(),
                    value: Branch {
                        name: name.to_owned(),
                        remote: None,
                    },
                })
            })
        });
        Picker::open(picker, Some(t!("dialog.new_worktree").into()), window, cx);
    }

    /// Asks before deleting a worktree, warning about uncommitted changes
    /// and the sessions that will close.
    pub(super) fn confirm_delete_worktree(
        &mut self,
        project: PathBuf,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = git::has_uncommitted_changes(&dir);
        let mut description = vec![
            match &status {
                Ok(true) => t!("dialog.worktree_dirty"),
                Ok(false) => t!("dialog.worktree_clean"),
                Err(error) => t!("dialog.worktree_status_unknown", error = error),
            },
            t!("dialog.branch_kept"),
        ];
        let sessions = self
            .workspace
            .sessions
            .iter()
            .filter(|session| session.dir == dir)
            .count();
        if sessions > 0 {
            description.push(t!("dialog.sessions_closed", count = sessions));
        }
        let description = description.join(" ");
        let description = if self.has_unsaved_in(&dir, cx) {
            format!("{description} {}", t!("files.unsaved_worktree"))
        } else {
            description
        };
        let force = !matches!(status, Ok(false));
        let title = t!("dialog.delete_worktree", name = directory_name(&dir)).into_owned();
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (view, project, dir) = (view.clone(), project.clone(), dir.clone());
            alert
                .confirm()
                .title(title.clone())
                .description(description.clone())
                .ok_text(t!("action.delete"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.delete_worktree(project.clone(), dir.clone(), force, window, cx);
                    })
                    .ok();
                    true
                })
        });
    }

    /// Asks before ending a session's agent.
    pub(super) fn confirm_close_session(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.workspace.session(id) else {
            return;
        };
        let title = t!(
            "dialog.close_session",
            agent = self.session_name(record),
            place = self.place_name(record)
        )
        .into_owned();
        // Agents Koi integrates with keep their conversation in Past Conversations.
        let is_agent = !self.running.get(&id).is_some_and(|running| running.shell)
            && self
                .agent(&record.agent)
                .and_then(Integration::for_agent)
                .is_some();
        let detail = if is_agent {
            t!("dialog.close_agent_detail")
        } else {
            t!("dialog.close_program_detail")
        };
        let detail = if self.has_unsaved_in(&record.dir, cx) {
            format!("{detail} {}", t!("files.unsaved_session"))
        } else {
            detail.into_owned()
        };
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title(title.clone())
                .description(detail.clone())
                .ok_text(t!("action.close_session"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.close_session(id, window, cx))
                        .ok();
                    true
                })
        });
    }

    /// Removes a project from the list, asking first if sessions would close.
    pub(super) fn confirm_remove_project(
        &mut self,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sessions = self
            .workspace
            .sessions
            .iter()
            .filter(|session| session.project == root)
            .count();
        if sessions == 0 {
            return self.remove_project(&root, window, cx);
        }
        let title = t!("dialog.remove_project", name = directory_name(&root)).into_owned();
        let description = [
            t!("dialog.sessions_closed", count = sessions),
            t!("dialog.disk_untouched"),
        ]
        .join(" ");
        let description = if self
            .workspace
            .sessions
            .iter()
            .any(|session| session.project == root && self.has_unsaved_in(&session.dir, cx))
        {
            format!("{description} {}", t!("files.unsaved_session"))
        } else {
            description
        };
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (view, root) = (view.clone(), root.clone());
            alert
                .confirm()
                .title(title.clone())
                .description(description.clone())
                .ok_text(t!("action.remove"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.remove_project(&root, window, cx))
                        .ok();
                    true
                })
        });
    }
}
