//! The session list: projects, their worktrees and the sessions in each.
//! Projects, and sessions within a worktree, can be dragged into another
//! order.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use koi_core::git::Worktree;
use koi_core::{Agent, Project, SessionId, SessionRecord};
use rust_i18n::t;

use super::drag::{self, Dragged};
use super::{AddProject, WorkspaceView};

/// Narrowest the menu for starting an agent gets.
const START_MENU_WIDTH: Pixels = px(180.);
/// Hover group of a list row whose secondary buttons appear on hover.
const ROW_GROUP: &str = "sidebar-row";

/// Holds a row's secondary buttons, hidden until the row is hovered unless
/// `shown`. Hidden buttons keep their space so the row does not reflow.
fn hover_actions(shown: bool) -> Div {
    h_flex().flex_none().gap_1().when(!shown, |actions| {
        actions
            .invisible()
            .group_hover(ROW_GROUP, |style| style.visible())
    })
}

/// A project row being dragged onto another project.
#[derive(Clone)]
struct ProjectItem(PathBuf);

/// A session row being dragged onto another session in the same directory.
#[derive(Clone)]
struct SessionItem {
    id: SessionId,
    dir: PathBuf,
}

/// One line of the list.
enum Row<'a> {
    Project(&'a Project),
    /// Why the project's worktrees could not be listed.
    Problem(&'a str),
    Worktree {
        project: &'a Path,
        worktree: &'a Worktree,
    },
    Session(&'a SessionRecord),
}

impl WorkspaceView {
    /// The list in display order.
    fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        for project in self
            .workspace
            .projects
            .iter()
            .filter(|project| !project.hidden)
        {
            rows.push(Row::Project(project));
            let sessions = || {
                self.workspace
                    .sessions
                    .iter()
                    .filter(|session| session.project == project.root)
            };
            let mut listed: Vec<&Path> = Vec::new();
            match self.worktrees.get(&project.root) {
                Some(Ok(trees)) => {
                    for tree in trees {
                        rows.push(Row::Worktree {
                            project: &project.root,
                            worktree: tree,
                        });
                        rows.extend(sessions().filter(|s| s.dir == tree.path).map(Row::Session));
                        listed.push(&tree.path);
                    }
                }
                Some(Err(problem)) => rows.push(Row::Problem(problem)),
                None => {}
            }
            // Sessions whose directory is not (or no longer) a listed worktree.
            rows.extend(
                sessions()
                    .filter(|session| !listed.contains(&session.dir.as_path()))
                    .map(Row::Session),
            );
        }
        rows
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(ix, row)| match row {
                Row::Project(project) => self.render_project(ix, project, cx),
                Row::Problem(problem) => div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(problem.to_owned())
                    .into_any_element(),
                Row::Worktree { project, worktree } => {
                    self.render_worktree(ix, project, worktree, cx)
                }
                Row::Session(session) => self.render_session(ix, session, cx),
            })
            .collect();
        let menu = self.render_sidebar_menu(cx);
        let theme = cx.theme();
        v_flex()
            .size_full()
            .flex_none()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t!("workspace.projects")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("sidebar-add-project")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .tooltip(t!("action.add_project"))
                                    .cursor_pointer()
                                    .on_click(|_, window, cx| {
                                        window.dispatch_action(Box::new(AddProject), cx)
                                    }),
                            )
                            .child(menu),
                    ),
            )
            .child(
                div()
                    .id("sidebar-rows")
                    .test_support()
                    .flex_1()
                    .overflow_y_scroll()
                    .pb_2()
                    .children(rows),
            )
    }

    fn render_project(&self, ix: usize, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let root = project.root.clone();
        let hide = {
            let root = root.clone();
            cx.listener(move |this, _, window, cx| this.set_project_hidden(&root, true, window, cx))
        };
        let new_worktree = {
            let root = root.clone();
            cx.listener(move |this, _, window, cx| {
                this.open_new_worktree_dialog(root.clone(), window, cx)
            })
        };
        let name: SharedString = project.name().into();
        h_flex()
            .id(("project", ix))
            .group(ROW_GROUP)
            .on_drag(
                Dragged {
                    item: ProjectItem(root.clone()),
                    name: name.clone(),
                },
                drag::preview,
            )
            .drag_over::<Dragged<ProjectItem>>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop({
                let root = root.clone();
                cx.listener(move |this, dragged: &Dragged<ProjectItem>, window, cx| {
                    if this.workspace.move_project(&dragged.item.0, &root) {
                        this.persist(window, cx);
                        cx.notify();
                    }
                })
            })
            .pl_3()
            .pr_2()
            .pt_3()
            .pb_1()
            .gap_1p5()
            .child(
                Icon::new(gpui_kit::assets::IconName::FolderOpen)
                    .small()
                    .flex_none()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .child(name),
            )
            .child(
                hover_actions(false)
                    // A plain folder has no branches to make worktrees of.
                    .children((!self.is_folder(&project.root)).then(|| {
                        Button::new(("new-worktree", ix))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip(t!("action.new_worktree"))
                            .cursor_pointer()
                            .on_click(new_worktree)
                    }))
                    .child(
                        Button::new(("hide-project", ix))
                            .ghost()
                            .xsmall()
                            .icon(IconName::EyeOff)
                            .tooltip(t!("action.hide_project"))
                            .cursor_pointer()
                            .on_click(hide),
                    )
                    .child(
                        Button::new(("remove-project", ix))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("action.remove_project"))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_remove_project(root.clone(), window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_worktree(
        &self,
        ix: usize,
        project: &Path,
        worktree: &Worktree,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (icon, label) = if self.is_folder(project) {
            (
                gpui_kit::assets::IconName::Folder,
                t!("workspace.not_git").into_owned(),
            )
        } else {
            let branch = worktree
                .branch
                .clone()
                .unwrap_or_else(|| t!("workspace.detached").into_owned());
            (gpui_kit::assets::IconName::GitBranch, branch)
        };
        let view = cx.entity().downgrade();
        let agents = self.agents.clone();
        let (project, dir) = (project.to_owned(), worktree.path.clone());
        let start_menu = {
            let (project, dir) = (project.clone(), dir.clone());
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<_>| {
                let item = |agent: &Agent, icon: gpui_kit::assets::IconName| {
                    let (view, project, dir, id) =
                        (view.clone(), project.clone(), dir.clone(), agent.id.clone());
                    PopupMenuItem::new(agent.name.clone()).icon(icon).on_click(
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.start_agent(
                                    &project,
                                    dir.clone(),
                                    &id,
                                    Vec::new(),
                                    window,
                                    cx,
                                );
                            })
                            .ok();
                        },
                    )
                };
                // Agents first, then plain shells below a separator.
                let (shells, programs): (Vec<&Agent>, Vec<&Agent>) =
                    agents.iter().partition(|agent| agent.command.is_empty());
                let menu = programs
                    .into_iter()
                    .fold(menu.min_w(START_MENU_WIDTH), |menu, agent| {
                        menu.item(item(agent, gpui_kit::assets::IconName::Bot))
                    });
                let menu = if shells.is_empty() {
                    menu
                } else {
                    menu.separator()
                };
                shells.into_iter().fold(menu, |menu, agent| {
                    menu.item(item(agent, gpui_kit::assets::IconName::SquareTerminal))
                })
            }
        };
        h_flex()
            .pl_4()
            .pr_2()
            .py_0p5()
            .gap_1()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(icon).xsmall().flex_none())
            .child(div().flex_1().min_w_0().truncate().text_xs().child(label))
            .children(
                self.change_count(&worktree.path)
                    .filter(|&count| count > 0)
                    .map(|count| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child(t!("workspace.change_count", count = count))
                    }),
            )
            .child({
                let (project, dir) = (project.clone(), dir.clone());
                Button::new(("history", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::BookOpen)
                    .tooltip(t!("action.history"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_history(project.clone(), dir.clone(), window, cx);
                    }))
            })
            .child(
                Button::new(("start-agent", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Play)
                    .tooltip(t!("action.start_agent"))
                    .cursor_pointer()
                    .dropdown_menu(start_menu),
            )
            .children((!worktree.is_main).then(|| {
                Button::new(("delete-worktree", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Delete)
                    .tooltip(t!("action.delete_worktree"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_delete_worktree(project.clone(), dir.clone(), window, cx);
                    }))
            }))
            .into_any_element()
    }

    fn render_session(
        &self,
        ix: usize,
        session: &SessionRecord,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = session.id;
        let detail = if session.label.is_some() {
            format!(
                "{} · {}",
                self.agent_name(session),
                self.place_name(session)
            )
        } else {
            self.running
                .get(&id)
                .and_then(|running| running.title.clone())
                .unwrap_or_else(|| self.place_name(session))
        };
        let name: SharedString = self.session_name(session).into();
        let status = self.status(id, cx);
        let unseen = self.is_unseen(id);
        let active = self.workspace.active == Some(id);
        let theme = cx.theme();
        v_flex()
            .id(("session", ix))
            .group(ROW_GROUP)
            .mx_2()
            .px_2()
            .py_1()
            .mb_1()
            .gap_0p5()
            .border_1()
            .border_color(if active {
                theme.sidebar_border
            } else {
                theme.sidebar_border.opacity(0.)
            })
            .rounded_md()
            .cursor_pointer()
            .when(active, |row| {
                row.bg(theme.sidebar_accent)
                    .text_color(theme.sidebar_accent_foreground)
            })
            .when(!active, |row| {
                row.hover(|row| row.bg(theme.sidebar_accent.opacity(0.5)))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
            .on_drag(
                Dragged {
                    item: SessionItem {
                        id,
                        dir: session.dir.clone(),
                    },
                    name: name.clone(),
                },
                drag::preview,
            )
            .drag_over::<Dragged<SessionItem>>({
                let dir = session.dir.clone();
                move |style, dragged, _, cx| {
                    if dragged.item.dir == dir {
                        style.bg(cx.theme().drop_target)
                    } else {
                        style
                    }
                }
            })
            .on_drop(
                cx.listener(move |this, dragged: &Dragged<SessionItem>, window, cx| {
                    if this.workspace.move_session(dragged.item.id, id) {
                        this.persist(window, cx);
                        cx.notify();
                    }
                }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(status.dot(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .child(name),
                    )
                    .when(unseen, |row| {
                        row.child(div().size_1p5().flex_none().rounded_full().bg(theme.danger))
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(status.label()),
                    )
                    .child(
                        hover_actions(active)
                            .child(
                                Button::new(("rename-session", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(gpui_kit::assets::IconName::Pencil)
                                    .tooltip(t!("action.rename_session"))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_rename_session_dialog(id, window, cx);
                                    })),
                            )
                            .child(
                                Button::new(("close-session", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip(t!("action.close_session"))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.confirm_close_session(id, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(detail),
                    )
                    .child(self.render_session_metrics(id, cx)),
            )
            .into_any_element()
    }
}
