//! Shortcuts the user can change in the settings, with their defaults.
//!
//! Every changeable binding is declared here, so the settings window, the
//! menus and the keymap agree. Bindings that are not meant to change, such
//! as the terminal keeping Tab, stay with their modules.

use std::collections::BTreeMap;

use gpui_kit::*;

use crate::{app_menu, terminal, workspace};

/// A command whose shortcut can be changed.
pub struct Shortcut {
    /// Translation key of the command's name.
    pub label: &'static str,
    /// The action's name, which keys the user's choice in the settings.
    pub name: &'static str,
    pub default: &'static str,
    context: Option<&'static str>,
    bind: fn(&str, Option<&str>) -> KeyBinding,
}

fn shortcut<A: Action + Default>(
    label: &'static str,
    default: &'static str,
    context: Option<&'static str>,
) -> Shortcut {
    Shortcut {
        label,
        name: A::name_for_type(),
        default,
        context,
        bind: |keys, context| KeyBinding::new(keys, A::default(), context),
    }
}

/// All changeable shortcuts, in the order the settings window lists them.
pub fn shortcuts() -> Vec<Shortcut> {
    let workspace = Some(workspace::CONTEXT);
    vec![
        shortcut::<app_menu::OpenSettings>("action.settings", "cmd-,", None),
        shortcut::<app_menu::Quit>("action.quit", "cmd-q", None),
        shortcut::<app_menu::Hide>("action.hide", "cmd-h", None),
        shortcut::<app_menu::HideOthers>("action.hide_others", "alt-cmd-h", None),
        shortcut::<app_menu::Minimize>("action.minimize", "cmd-m", None),
        shortcut::<app_menu::CloseWindow>("action.close_window", "shift-cmd-w", None),
        shortcut::<workspace::AddProject>("action.add_project", "cmd-o", workspace),
        shortcut::<workspace::QuickOpen>("action.quick_open", "cmd-p", workspace),
        shortcut::<workspace::ShowOverview>("action.show_overview", "cmd-0", workspace),
        shortcut::<workspace::NextTab>("action.next_tab", "cmd-shift-]", workspace),
        shortcut::<workspace::PreviousTab>("action.previous_tab", "cmd-shift-[", workspace),
        shortcut::<workspace::CloseTab>("action.close_tab", "cmd-w", workspace),
        shortcut::<workspace::ToggleChanges>("action.toggle_changes", "cmd-shift-g", workspace),
        shortcut::<workspace::ToggleFiles>("action.toggle_files", "cmd-shift-e", workspace),
        shortcut::<workspace::Save>("files.save", "cmd-s", Some(workspace::FILES_CONTEXT)),
        shortcut::<terminal::Copy>("action.copy", "cmd-c", Some(terminal::CONTEXT)),
        shortcut::<terminal::Paste>("action.paste", "cmd-v", Some(terminal::CONTEXT)),
    ]
}

impl Shortcut {
    /// The keys in effect: the user's choice if it is valid, else the default.
    pub fn keys(&self, overrides: &BTreeMap<String, String>) -> String {
        overrides
            .get(self.name)
            .filter(|keys| is_valid(keys))
            .map_or_else(|| self.default.to_owned(), Clone::clone)
    }
}

/// Whether `keys` is a shortcut, such as `cmd-shift-k`, or a sequence of
/// them separated by spaces.
pub fn is_valid(keys: &str) -> bool {
    !keys.trim().is_empty()
        && keys
            .split_whitespace()
            .all(|keystroke| Keystroke::parse(keystroke).is_ok())
}

/// Binds the shortcuts with `overrides` applied, replacing their earlier
/// bindings and keeping every other binding as it was.
pub fn bind(overrides: &BTreeMap<String, String>, cx: &mut App) {
    let shortcuts = shortcuts();
    let keymap = cx.key_bindings();
    let mut keymap = keymap.borrow_mut();
    let kept: Vec<KeyBinding> = keymap
        .bindings()
        .filter(|binding| {
            shortcuts
                .iter()
                .all(|shortcut| shortcut.name != binding.action().name())
        })
        .cloned()
        .collect();
    keymap.clear();
    keymap.add_bindings(kept);
    keymap.add_bindings(
        shortcuts
            .iter()
            .map(|shortcut| (shortcut.bind)(&shortcut.keys(overrides), shortcut.context)),
    );
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, is_valid, shortcuts};
    use std::collections::HashSet;

    #[test]
    fn defaults_are_valid_and_distinct_per_context() {
        let mut seen = HashSet::new();
        for shortcut in shortcuts() {
            assert!(is_valid(shortcut.default), "{}", shortcut.name);
            assert!(
                seen.insert((shortcut.context, shortcut.default)),
                "{} is taken",
                shortcut.default
            );
        }
    }

    #[test]
    fn every_shortcut_has_a_translated_name() {
        for shortcut in shortcuts() {
            let name = rust_i18n::t!(shortcut.label, locale = "en", app = "Koi");
            assert_ne!(
                name, shortcut.label,
                "missing translation for {}",
                shortcut.label
            );
        }
    }

    #[test]
    fn invalid_overrides_fall_back_to_the_default() {
        let quick_open = shortcuts()
            .into_iter()
            .find(|s| s.name == "workspace::QuickOpen")
            .unwrap();
        let mut overrides = BTreeMap::new();
        assert_eq!(quick_open.keys(&overrides), "cmd-p");
        overrides.insert(quick_open.name.to_owned(), "cmd-k".to_owned());
        assert_eq!(quick_open.keys(&overrides), "cmd-k");
        overrides.insert(quick_open.name.to_owned(), "cmd-nonsense-key-".to_owned());
        assert_eq!(quick_open.keys(&overrides), "cmd-p");
    }

    #[test]
    fn validates_keys() {
        assert!(is_valid("cmd-shift-k"));
        assert!(is_valid("cmd-k cmd-s"));
        assert!(!is_valid(""));
        assert!(!is_valid("   "));
    }
}
