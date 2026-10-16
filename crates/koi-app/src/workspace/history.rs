//! Past conversations in a worktree, listed so one can be resumed in a new
//! session.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use koi_core::Agent;
use koi_core::history::{PastSession, past_sessions};
use koi_core::integration::Integration;
use rust_i18n::t;

use super::WorkspaceView;

/// Conversations listed per agent.
const LIMIT: usize = 30;

/// A past conversation and the agent that resumes it.
struct Entry {
    agent: String,
    agent_name: String,
    session: PastSession,
}

/// How long ago something happened, at the precision the list shows.
#[derive(Debug, PartialEq, Eq)]
enum Age {
    JustNow,
    Minutes(u64),
    Hours(u64),
    Days(u64),
}

impl Age {
    fn of(elapsed: Duration) -> Self {
        let minutes = elapsed.as_secs() / 60;
        match minutes {
            0 => Self::JustNow,
            1..60 => Self::Minutes(minutes),
            60..1440 => Self::Hours(minutes / 60),
            _ => Self::Days(minutes / 1440),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::JustNow => t!("age.just_now"),
            Self::Minutes(count) => t!("age.minutes", count = count),
            Self::Hours(count) => t!("age.hours", count = count),
            Self::Days(count) => t!("age.days", count = count),
        }
        .into_owned()
    }
}

impl WorkspaceView {
    /// Reads past conversations in `dir` in the background, then lists them.
    pub(super) fn open_history(
        &mut self,
        project: PathBuf,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Each kind of agent resumes with the first configured agent of that kind.
        let mut agents: Vec<(Agent, Integration)> = Vec::new();
        for agent in &self.agents {
            if let Some(integration) = Integration::for_agent(agent)
                && agents.iter().all(|(_, known)| *known != integration)
            {
                agents.push((agent.clone(), integration));
            }
        }
        let home = self.paths.home().to_owned();
        let shell = self.shell.clone();
        let task = cx.background_spawn({
            let dir = dir.clone();
            async move {
                let mut entries: Vec<Entry> = agents
                    .into_iter()
                    .flat_map(|(agent, _)| {
                        past_sessions(&agent, &shell, &home, &dir, LIMIT)
                            .into_iter()
                            .map(move |session| Entry {
                                agent: agent.id.clone(),
                                agent_name: agent.name.clone(),
                                session,
                            })
                    })
                    .collect();
                entries.sort_by_key(|entry| std::cmp::Reverse(entry.session.updated));
                entries
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let entries = task.await;
            this.update_in(cx, |this, window, cx| {
                this.show_history(project, dir, entries, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn show_history(
        &mut self,
        project: PathBuf,
        dir: PathBuf,
        entries: Vec<Entry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = t!("dialog.history", name = self.place_of(&project, &dir)).into_owned();
        let view = cx.entity().downgrade();
        let entries = Rc::new(entries);
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme();
            let now = SystemTime::now();
            let rows = entries.iter().enumerate().map(|(ix, entry)| {
                let (view, project, dir) = (view.clone(), project.clone(), dir.clone());
                let (agent, resume) = (entry.agent.clone(), entry.session.resume_args.clone());
                let age = Age::of(
                    now.duration_since(entry.session.updated)
                        .unwrap_or_default(),
                );
                h_flex()
                    .id(("history", ix))
                    .px_2()
                    .py_1p5()
                    .gap_3()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|row| row.bg(theme.list_hover))
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        view.update(cx, |this, cx| {
                            this.start_agent(
                                &project,
                                dir.clone(),
                                &agent,
                                resume.clone(),
                                window,
                                cx,
                            );
                        })
                        .ok();
                    })
                    .child(
                        div()
                            .w_20()
                            .flex_none()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(entry.agent_name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(entry.session.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(age.label()),
                    )
            });
            let list = if entries.is_empty() {
                div()
                    .py_4()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(t!("dialog.no_history"))
                    .into_any_element()
            } else {
                v_flex()
                    .id("history-list")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .children(rows)
                    .into_any_element()
            };
            dialog.title(title.clone()).w(px(560.)).child(list)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Age, Duration};

    #[test]
    fn age_precision() {
        assert_eq!(Age::of(Duration::from_secs(30)), Age::JustNow);
        assert_eq!(Age::of(Duration::from_secs(60 * 59)), Age::Minutes(59));
        assert_eq!(Age::of(Duration::from_secs(60 * 60 * 5)), Age::Hours(5));
        assert_eq!(Age::of(Duration::from_secs(60 * 60 * 50)), Age::Days(2));
    }
}
