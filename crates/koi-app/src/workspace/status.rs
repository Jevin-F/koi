//! How a session's state is shown: a colored dot and a label.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use rust_i18n::t;

/// What a session is doing, as far as Koi knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Running a turn.
    Working,
    /// Blocked on the user, for example a permission prompt.
    Waiting,
    /// Finished its turn.
    Idle,
    /// Finished its turn, and the user has not looked since.
    Finished,
    /// Running, with no activity reporting.
    Running,
    /// The agent exited and an interactive shell has taken over.
    Shell,
    Exited,
    /// Exited with a non-zero exit code.
    Failed,
}

impl Status {
    pub fn label(self) -> SharedString {
        match self {
            Self::Working => t!("status.working"),
            Self::Waiting => t!("status.waiting"),
            Self::Idle => t!("status.idle"),
            Self::Finished => t!("status.finished"),
            Self::Running => t!("status.running"),
            Self::Shell => t!("status.shell"),
            Self::Exited => t!("status.exited"),
            Self::Failed => t!("status.failed"),
        }
        .into()
    }

    fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::Working | Self::Running => theme.info,
            Self::Waiting => theme.warning,
            Self::Idle | Self::Finished => theme.success,
            Self::Shell => theme.muted_foreground,
            Self::Exited => theme.muted_foreground.opacity(0.4),
            Self::Failed => theme.danger,
        }
    }

    /// A small dot in the status color.
    pub fn dot(self, cx: &App) -> Div {
        div().size_2().flex_none().rounded_full().bg(self.color(cx))
    }
}
