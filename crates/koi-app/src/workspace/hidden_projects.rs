//! Hiding projects from the sidebar, the tab bar, the overview and the
//! running count. Their sessions keep running and stay in quick open; the
//! sidebar's menu shows them again.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{IconName, Sizable};
use gpui_kit::*;
use rust_i18n::t;

use super::WorkspaceView;

impl WorkspaceView {
    pub(super) fn set_project_hidden(
        &mut self,
        root: &Path,
        hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.set_project_hidden(root, hidden) {
            self.persist(window, cx);
            cx.notify();
        }
    }

    fn show_all_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.show_all_projects() {
            self.persist(window, cx);
            cx.notify();
        }
    }

    /// The sidebar's "more" button. Its menu shows all projects or picks
    /// which ones are listed, in alphabetical order.
    pub(super) fn render_sidebar_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity().downgrade();
        let projects: Vec<(PathBuf, String, bool)> = self
            .workspace
            .projects_by_name()
            .into_iter()
            .map(|project| (project.root.clone(), project.name(), project.hidden))
            .collect();
        Button::new("sidebar-menu")
            .ghost()
            .xsmall()
            .icon(IconName::Ellipsis)
            .tooltip(t!("action.sidebar_menu"))
            .cursor_pointer()
            .dropdown_menu(move |menu, window, cx| {
                let (view, projects) = (view.clone(), projects.clone());
                menu.submenu(t!("action.show_projects"), window, cx, move |menu, _, _| {
                    let show_all = {
                        let view = view.clone();
                        PopupMenuItem::new(t!("action.show_all_projects"))
                            .disabled(projects.iter().all(|(_, _, hidden)| !hidden))
                            .on_click(move |_, window, cx| {
                                view.update(cx, |this, cx| this.show_all_projects(window, cx))
                                    .ok();
                            })
                    };
                    projects.iter().fold(
                        menu.item(show_all).separator(),
                        |menu, (root, name, hidden)| {
                            let (view, root, hidden) = (view.clone(), root.clone(), *hidden);
                            menu.item(PopupMenuItem::new(name.clone()).checked(!hidden).on_click(
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.set_project_hidden(&root, !hidden, window, cx)
                                    })
                                    .ok();
                                },
                            ))
                        },
                    )
                })
            })
            .into_any_element()
    }
}
