//! A worktree's files: a folder tree beside an editor for the open file.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::base::Disableable;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use koi_core::file_language::language_for;
use koi_core::file_tabs::FileTabs;
use koi_core::file_tree::{FileTree, TreeRow, ancestors};
use koi_core::project_files;

use super::file_icon::{file_icon, folder_icon};
use rust_i18n::t;

pub const CONTEXT: &str = "Files";
const TREE_WIDTH: Pixels = px(260.);
const ROW_HEIGHT: Pixels = px(24.);
const INDENT: Pixels = px(12.);
/// Larger files are not opened; the editor is meant for quick looks and edits.
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

actions!(files, [Save]);

/// The file shown in the editor.
struct OpenFile {
    editor: Entity<EditorState>,
    /// Why the file is not shown, such as being binary.
    problem: Option<SharedString>,
    load_generation: u64,
    _events: Subscription,
}

pub struct FilesView {
    dir: PathBuf,
    window: AnyWindowHandle,
    tree: FileTree,
    expanded: HashSet<String>,
    rows: Vec<TreeRow>,
    tabs: FileTabs,
    open: HashMap<String, OpenFile>,
    next_load_generation: u64,
}

impl FilesView {
    pub fn new(dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            dir,
            window: window.window_handle(),
            tree: FileTree::default(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            tabs: FileTabs::default(),
            open: HashMap::new(),
            next_load_generation: 0,
        };
        this.reload(cx);
        this
    }

    /// Lists the files again and reloads the open file unless it has edits.
    /// Called when files in the worktree change.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        let task = cx.background_spawn(async move { project_files::files(&dir) });
        cx.spawn(async move |this, cx| {
            // An unreadable list keeps showing the previous one.
            let Ok(files) = task.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.tree = FileTree::new(files.iter().map(String::as_str));
                this.rows = this.tree.rows(&this.expanded);
                for path in this.tabs.paths().to_vec() {
                    if !this.tabs.is_dirty(&path) {
                        this.load_content(path, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens `path`, expanding the folders above it.
    pub fn reveal(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.expanded.extend(ancestors(&path));
        self.rows = self.tree.rows(&self.expanded);
        self.open_file(path, window, cx);
    }

    fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_owned());
        }
        self.rows = self.tree.rows(&self.expanded);
        cx.notify();
    }

    /// Opens or selects `path` without losing the state of other tabs.
    fn open_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.open(path.clone()) {
            cx.notify();
            return;
        }
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language_for(&path))
                .line_number(true)
                .soft_wrap(false)
        });
        let watched_path = path.clone();
        let events = cx.subscribe(&editor, move |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.tabs.set_dirty(&watched_path, true);
                cx.notify();
            }
        });
        self.open.insert(
            path.clone(),
            OpenFile {
                editor,
                problem: Some(t!("files.loading").into()),
                load_generation: 0,
                _events: events,
            },
        );
        cx.notify();
        self.load_content(path, cx);
    }

    /// Reads an open file without changing the selected tab.
    fn load_content(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(open) = self.open.get_mut(&path) else {
            return;
        };
        self.next_load_generation += 1;
        let generation = self.next_load_generation;
        open.load_generation = generation;
        let file = self.dir.join(&path);
        let task = cx.background_spawn(async move {
            let size = std::fs::metadata(&file)
                .map_err(|error| error.to_string())?
                .len();
            if size > MAX_FILE_SIZE {
                return Ok(None);
            }
            let bytes = std::fs::read(&file).map_err(|error| error.to_string())?;
            Ok(String::from_utf8(bytes).ok())
        });
        let window = self.window;
        cx.spawn(async move |this, cx| {
            let content: Result<Option<String>, String> = task.await;
            window
                .update(cx, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.show(path, generation, content, window, cx)
                    })
                    .ok();
                })
                .ok();
        })
        .detach();
    }

    fn show(
        &mut self,
        path: String,
        generation: u64,
        content: Result<Option<String>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.open.get_mut(&path) else {
            return;
        };
        if self.tabs.is_dirty(&path) || open.load_generation != generation {
            return;
        }
        open.problem = match content {
            Ok(Some(text)) => {
                // Rewriting unchanged text would move the cursor back to the top.
                let unchanged = open.editor.read(cx).value() == text;
                if !unchanged {
                    open.editor
                        .update(cx, |editor, cx| editor.set_value(text, window, cx));
                }
                None
            }
            Ok(None) => Some(t!("files.not_text").into()),
            Err(error) => Some(t!("files.unreadable", error = error).into()),
        };
        self.tabs.set_dirty(&path, false);
        cx.notify();
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().filter(|path| self.tabs.is_dirty(path)) else {
            return;
        };
        let Some(open) = self.open.get(path).filter(|open| open.problem.is_none()) else {
            return;
        };
        let text = open.editor.read(cx).value();
        let path = path.to_owned();
        match std::fs::write(self.dir.join(&path), text.as_bytes()) {
            Ok(()) => self.tabs.set_dirty(&path, false),
            Err(error) => {
                let message = t!("files.save_failed", error = error);
                window.push_notification(Notification::error(message), cx);
            }
        }
        cx.notify();
    }

    /// Whether any editor tab has changes that are only in memory.
    pub fn has_unsaved(&self) -> bool {
        self.tabs.has_unsaved()
    }

    fn close_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.tabs.close(path);
        self.open.remove(path);
        cx.notify();
    }

    fn request_close_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.is_dirty(&path) {
            self.close_file(&path, cx);
            return;
        }
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (view, path) = (view.clone(), path.clone());
            alert
                .confirm()
                .title(t!("files.close_unsaved", name = path).into_owned())
                .description(t!("files.close_unsaved_detail"))
                .ok_text(t!("files.discard"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| this.close_file(&path, cx)).ok();
                    true
                })
        });
    }

    fn render_row(&self, ix: usize, row: &TreeRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let selected = !row.is_dir && self.tabs.active() == Some(row.path.as_str());
        let chevron = row.is_dir.then_some(if row.expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        });
        let kind = if row.is_dir {
            folder_icon(row.expanded)
        } else {
            file_icon(&row.name)
        };
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        h_flex()
            .id(("tree-row", ix))
            .h(ROW_HEIGHT)
            .pl(INDENT * (row.depth as f32 + 0.5))
            .pr_2()
            .gap_1()
            .text_sm()
            .cursor_pointer()
            .when(selected, |item| item.bg(theme.list_active))
            .when(!selected, |item| {
                item.hover(|item| item.bg(theme.list_hover))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if is_dir {
                    this.toggle_folder(&path, cx);
                } else {
                    this.open_file(path.clone(), window, cx);
                }
            }))
            .child(div().w_4().flex_none().children(chevron))
            .child(
                Icon::new(kind)
                    .small()
                    .flex_none()
                    .text_color(theme.muted_foreground),
            )
            .child(div().min_w_0().truncate().child(row.name.clone()))
            .into_any_element()
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let notice = |text: SharedString| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
                .into_any_element()
        };
        let Some(path) = self.tabs.active() else {
            return notice(t!("files.pick").into());
        };
        let Some(open) = self.open.get(path) else {
            return notice(t!("files.pick").into());
        };
        let body = match &open.problem {
            Some(problem) => notice(problem.clone()),
            None => Editor::new(&open.editor)
                .bordered(false)
                .size_full()
                .into_any_element(),
        };
        let tabs: Vec<AnyElement> = self
            .tabs
            .paths()
            .iter()
            .enumerate()
            .map(|(ix, path)| {
                let active = self.tabs.active() == Some(path.as_str());
                let dirty = self.tabs.is_dirty(path);
                let name = path.rsplit('/').next().unwrap_or(path).to_owned();
                let selected_path = path.clone();
                let closing_path = path.clone();
                let tooltip_path = path.clone();
                h_flex()
                    .id(("file-tab", ix))
                    .h_full()
                    .max_w(px(190.))
                    .flex_none()
                    .px_2()
                    .gap_1()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(theme.border)
                    .when(active, |tab| {
                        tab.bg(theme.background).text_color(theme.foreground)
                    })
                    .when(!active, |tab| {
                        tab.text_color(theme.muted_foreground)
                            .hover(|tab| tab.bg(theme.list_hover))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_file(selected_path.clone(), window, cx)
                    }))
                    .tooltip(move |window, cx| Tooltip::new(tooltip_path.clone()).build(window, cx))
                    .child(Icon::new(file_icon(&name)).small().flex_none())
                    .child(div().min_w_0().truncate().text_xs().child(name))
                    .children(dirty.then(|| div().size_1p5().rounded_full().bg(theme.warning)))
                    .child(
                        Button::new(("close-file-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("files.close_tab"))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.request_close_file(closing_path.clone(), window, cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(34.))
                    .flex_none()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_x_scrollbar()
                            .child(h_flex().h_full().children(tabs)),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(self.tabs.paths().len().to_string()),
                    )
                    .child(
                        Button::new("save-file")
                            .ghost()
                            .xsmall()
                            .icon(gpui_kit::assets::IconName::Save)
                            .tooltip(t!("files.save"))
                            .disabled(!self.tabs.is_dirty(path))
                            .when(self.tabs.is_dirty(path), |button| button.cursor_pointer())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save(&Save, window, cx)),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

impl Render for FilesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tree = uniform_list(
            "file-tree",
            self.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|ix| {
                        let row = this.rows[ix].clone();
                        this.render_row(ix, &row, cx)
                    })
                    .collect()
            }),
        )
        .size_full();
        h_flex()
            .size_full()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::save))
            .child(
                div()
                    .w(TREE_WIDTH)
                    .flex_none()
                    .h_full()
                    .py_1()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(tree),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_editor(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::FilesView;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};

    #[gpui_kit::test]
    fn opens_files_from_a_window_event(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("koi-editor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("first.rs"), "fn first() {}\n").unwrap();
        std::fs::write(dir.join("second.py"), "print('second')\n").unwrap();

        let (window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| FilesView::new(dir.clone(), window, cx))
            })
            .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| view.open_file("first.rs".into(), window, cx));
            view.update(cx, |view, cx| {
                view.open_file("second.py".into(), window, cx)
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            let files = view.read(cx);
            assert_eq!(files.tabs.paths(), ["first.rs", "second.py"]);
            assert_eq!(files.tabs.active(), Some("second.py"));
            assert_eq!(
                files.open["first.rs"].editor.read(cx).value(),
                "fn first() {}\n"
            );
            assert_eq!(
                files.open["second.py"].editor.read(cx).value(),
                "print('second')\n"
            );
            assert_eq!(
                files.open["first.rs"].editor.read(cx).language_name(),
                "rust"
            );
        });
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| view.open_file("first.rs".into(), window, cx));
            window.render_frame(cx);
            let editor = view.read(cx).open["first.rs"].editor.clone();
            editor.update(cx, |editor, cx| editor.focus(window, cx));
            window.render_frame(cx);
            window.input("edited", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.open_file("second.py".into(), window, cx)
            });
            view.update(cx, |view, cx| view.open_file("first.rs".into(), window, cx));
        })
        .unwrap();
        cx.update(|cx| {
            let files = view.read(cx);
            assert_eq!(files.tabs.active(), Some("first.rs"));
            assert!(
                files.tabs.is_dirty("first.rs"),
                "{}",
                files.open["first.rs"].editor.read(cx).value()
            );
            assert!(
                files.open["first.rs"]
                    .editor
                    .read(cx)
                    .value()
                    .contains("edited")
            );
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
