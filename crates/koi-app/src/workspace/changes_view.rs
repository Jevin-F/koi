//! A worktree's uncommitted changes: the changed files with a commit box,
//! the selected file's diff, and pushing to the remote.

use std::path::PathBuf;

use gpui_kit::base::Disableable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use koi_core::git::{self, ChangeKind, DiffLine, FileChange, LineKind, Tracking};
use rust_i18n::t;

use super::file_icon::file_icon;
use crate::settings::code_font;

const FILE_LIST_WIDTH: Pixels = px(280.);
const DIFF_LINE_HEIGHT: Pixels = px(20.);
const LINE_NUMBER_WIDTH: Pixels = px(44.);

/// A git operation in progress; others wait until it is done.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Committing,
    Pushing,
}

pub struct ChangesView {
    dir: PathBuf,
    /// Branch checked out in the worktree, for the header.
    branch: String,
    files: Vec<FileChange>,
    selected: Option<String>,
    /// Diff of the selected file, once loaded.
    diff: Option<Result<Vec<DiffLine>, String>>,
    tracking: Option<Tracking>,
    busy: Option<Busy>,
    message: Entity<InputState>,
    _message_events: Subscription,
}

impl ChangesView {
    pub fn new(dir: PathBuf, branch: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| InputState::new(window, cx).placeholder(t!("changes.message")));
        let message_events = cx.subscribe_in(&message, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.commit(window, cx);
            }
        });
        let mut this = Self {
            dir,
            branch,
            files: Vec::new(),
            selected: None,
            diff: None,
            tracking: None,
            busy: None,
            message,
            _message_events: message_events,
        };
        this.refresh_tracking(cx);
        this
    }

    /// Shows the latest list of changed files, keeping the selection if the
    /// file is still changed.
    pub fn set_files(&mut self, files: Vec<FileChange>, cx: &mut Context<Self>) {
        let keep = self
            .selected
            .clone()
            .filter(|path| files.iter().any(|file| &file.path == path));
        self.files = files;
        let selected = keep.or_else(|| self.files.first().map(|file| file.path.clone()));
        self.select(selected, cx);
        self.refresh_tracking(cx);
    }

    pub fn set_branch(&mut self, branch: String) {
        self.branch = branch;
    }

    fn select(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.selected = path.clone();
        let Some(file) = path.and_then(|path| self.files.iter().find(|file| file.path == path))
        else {
            self.diff = None;
            cx.notify();
            return;
        };
        let (dir, file) = (self.dir.clone(), file.clone());
        let path = file.path.clone();
        let task = cx.background_spawn(async move { git::file_diff(&dir, &file) });
        cx.spawn(async move |this, cx| {
            let diff = task.await.map_err(|error| error.to_string());
            this.update(cx, |this, cx| {
                // A diff for a file selected earlier may arrive late; drop it.
                if this.selected.as_ref() == Some(&path) {
                    this.diff = Some(diff);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn refresh_tracking(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        let task = cx.background_spawn(async move { git::tracking(&dir) });
        cx.spawn(async move |this, cx| {
            // Without readable tracking information the header just omits it.
            let tracking = task.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.tracking = tracking;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_owned();
        if message.is_empty() || self.files.is_empty() || self.busy.is_some() {
            return;
        }
        let dir = self.dir.clone();
        let task = cx.background_spawn(async move { git::commit_all(&dir, &message) });
        self.run(Busy::Committing, task, window, cx, |this, window, cx| {
            this.message
                .update(cx, |input, cx| input.set_value("", window, cx));
        });
    }

    fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let dir = self.dir.clone();
        let task = cx.background_spawn(async move { git::push(&dir) });
        self.run(Busy::Pushing, task, window, cx, |_, window, cx| {
            window.push_notification(Notification::success(t!("changes.pushed")), cx);
        });
    }

    /// Runs a git operation in the background, then reports failures and
    /// refreshes. The file list refreshes through the workspace's watchers.
    fn run<T: 'static>(
        &mut self,
        busy: Busy,
        task: Task<git::Result<T>>,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_success: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.busy = Some(busy);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(_) => on_success(this, window, cx),
                    Err(git::Error::NoRemote) => {
                        window.push_notification(Notification::error(t!("error.no_remote")), cx);
                    }
                    Err(error) => {
                        let message = t!("error.git", error = error);
                        window.push_notification(Notification::error(message), cx);
                    }
                }
                this.refresh_tracking(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tracking = match &self.tracking {
            Some(tracking) => {
                t!(
                    "changes.tracking",
                    upstream = tracking.upstream,
                    ahead = tracking.ahead,
                    behind = tracking.behind
                )
            }
            None => t!("changes.no_upstream"),
        };
        let push_label = match &self.tracking {
            Some(tracking) if tracking.ahead > 0 => {
                t!("changes.push_count", count = tracking.ahead)
            }
            _ => t!("changes.push"),
        };
        h_flex()
            .px_3()
            .py_2()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.branch.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tracking),
            )
            .child(
                Button::new("push")
                    .small()
                    .label(push_label)
                    .loading(self.busy == Some(Busy::Pushing))
                    .disabled(self.busy.is_some())
                    .when(self.busy.is_none(), |button| button.cursor_pointer())
                    .on_click(cx.listener(|this, _, window, cx| this.push(window, cx))),
            )
    }

    fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.files.iter().enumerate().map(|(ix, file)| {
            let (letter, color) = match file.kind {
                ChangeKind::Added => ("A", theme.success),
                ChangeKind::Untracked => ("U", theme.success),
                ChangeKind::Modified => ("M", theme.warning),
                ChangeKind::Deleted => ("D", theme.danger),
                ChangeKind::Renamed => ("R", theme.info),
                ChangeKind::Conflicted => ("!", theme.danger),
            };
            let (folder, name) = match file.path.rsplit_once('/') {
                Some((folder, name)) => (folder.to_owned(), name.to_owned()),
                None => (String::new(), file.path.clone()),
            };
            let selected = self.selected.as_ref() == Some(&file.path);
            let path = file.path.clone();
            h_flex()
                .id(("changed-file", ix))
                .px_2()
                .py_0p5()
                .gap_2()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.list_active))
                .when(!selected, |row| row.hover(|row| row.bg(theme.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| this.select(Some(path.clone()), cx)))
                .child(
                    div()
                        .w_3()
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(letter),
                )
                .child(
                    Icon::new(file_icon(&name))
                        .small()
                        .flex_none()
                        .text_color(theme.muted_foreground),
                )
                .child(div().flex_none().text_sm().child(name))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(folder),
                )
        });
        let commit_label = t!("changes.commit_count", count = self.files.len());
        v_flex()
            .w(FILE_LIST_WIDTH)
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .id("changed-files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_1()
                    .children(rows),
            )
            .child(
                v_flex()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(Input::new(&self.message))
                    .child(
                        Button::new("commit")
                            .primary()
                            .small()
                            .label(commit_label)
                            .loading(self.busy == Some(Busy::Committing))
                            .disabled(self.files.is_empty() || self.busy.is_some())
                            .when(!self.files.is_empty() && self.busy.is_none(), |button| {
                                button.cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    ),
            )
    }

    fn render_diff(&self, cx: &mut Context<Self>) -> AnyElement {
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
        match &self.diff {
            _ if self.files.is_empty() => notice(t!("changes.clean").into()),
            None => notice(SharedString::default()),
            Some(Err(error)) => notice(t!("error.git", error = error).into()),
            Some(Ok(lines)) if lines.is_empty() => notice(t!("changes.no_diff").into()),
            Some(Ok(lines)) => uniform_list(
                "diff",
                lines.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let Some(Ok(lines)) = &this.diff else {
                        return Vec::new();
                    };
                    lines[range]
                        .iter()
                        .map(|line| render_diff_line(line, cx))
                        .collect()
                }),
            )
            .size_full()
            .into_any_element(),
        }
    }
}

fn render_diff_line(line: &DiffLine, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (background, sign) = match line.kind {
        LineKind::Added => (Some(theme.success.opacity(0.15)), "+"),
        LineKind::Removed => (Some(theme.danger.opacity(0.15)), "-"),
        LineKind::Hunk => (Some(theme.muted), ""),
        LineKind::Context | LineKind::Note => (None, ""),
    };
    let number = |number: Option<u32>| {
        div()
            .w(LINE_NUMBER_WIDTH)
            .flex_none()
            .pr_2()
            .text_right()
            .text_color(theme.muted_foreground)
            .child(number.map_or_else(String::new, |number| number.to_string()))
    };
    let text_color = match line.kind {
        LineKind::Hunk | LineKind::Note => theme.muted_foreground,
        _ => theme.foreground,
    };
    h_flex()
        .h(DIFF_LINE_HEIGHT)
        .w_full()
        .font_family(code_font(cx).family)
        .text_xs()
        .whitespace_nowrap()
        .overflow_hidden()
        .when_some(background, |row, background| row.bg(background))
        .child(number(line.old))
        .child(number(line.new))
        .child(div().w_4().flex_none().text_color(text_color).child(sign))
        .child(
            div()
                .text_color(text_color)
                .child(line.text.replace('\t', "    ")),
        )
        .into_any_element()
}

impl Render for ChangesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(self.render_header(cx)).child(
            h_flex()
                .flex_1()
                .min_h_0()
                .child(self.render_files(cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_diff(cx)),
                ),
        )
    }
}
