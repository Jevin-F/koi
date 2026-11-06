//! The letter and color that stand for a git change wherever files are listed.

use gpui_kit::Hsla;
use gpui_kit::component::Theme;
use koi_core::git::ChangeKind;

/// A one-letter badge for `kind` and the color it is drawn in.
pub fn change_badge(kind: ChangeKind, theme: &Theme) -> (&'static str, Hsla) {
    match kind {
        ChangeKind::Added => ("A", theme.success),
        ChangeKind::Untracked => ("U", theme.success),
        ChangeKind::Modified => ("M", theme.warning),
        ChangeKind::Deleted => ("D", theme.danger),
        ChangeKind::Renamed => ("R", theme.info),
        ChangeKind::Conflicted => ("!", theme.danger),
    }
}
