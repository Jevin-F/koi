//! Menu bar and application-wide commands.

use gpui_kit::*;
use rust_i18n::t;

use crate::{APP_NAME, main_window, settings_view, terminal, workspace};

actions!(
    koi,
    [
        OpenSettings,
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        CloseWindow
    ]
);

/// Registers application commands. Their shortcuts are bound by
/// [`crate::shortcuts`]; the menu bar is built by [`set_menus`].
pub fn init(cx: &mut App) {
    cx.on_action(|_: &OpenSettings, cx| settings_view::open(cx));
    cx.on_action(|_: &Quit, cx| main_window::quit(cx));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(handle) = cx.active_window() {
            handle
                .update(cx, |_, window, _| window.minimize_window())
                .expect("active window is open");
        }
    });
    cx.on_action(|_: &CloseWindow, cx| main_window::close(cx));
}

/// Builds the menu bar in the current language, with the current shortcuts.
pub fn set_menus(cx: &mut App) {
    cx.set_menus(menus());
}

fn menus() -> Vec<Menu> {
    vec![
        Menu::new(APP_NAME).items([
            MenuItem::action(t!("action.settings"), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(t!("menu.services"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(t!("action.hide", app = APP_NAME), Hide),
            MenuItem::action(t!("action.hide_others"), HideOthers),
            MenuItem::action(t!("action.show_all"), ShowAll),
            MenuItem::separator(),
            MenuItem::action(t!("action.quit", app = APP_NAME), Quit),
        ]),
        Menu::new(t!("menu.edit")).items([
            MenuItem::action(t!("action.copy"), terminal::Copy),
            MenuItem::action(t!("action.paste"), terminal::Paste),
        ]),
        Menu::new(t!("menu.workspace")).items([
            MenuItem::action(t!("action.add_project"), workspace::AddProject),
            MenuItem::action(t!("action.quick_open"), workspace::QuickOpen),
            MenuItem::separator(),
            MenuItem::action(t!("action.show_overview"), workspace::ShowOverview),
            MenuItem::action(t!("action.toggle_changes"), workspace::ToggleChanges),
            MenuItem::action(t!("action.toggle_files"), workspace::ToggleFiles),
            MenuItem::action(t!("action.next_tab"), workspace::NextTab),
            MenuItem::action(t!("action.previous_tab"), workspace::PreviousTab),
        ]),
        Menu::new(t!("menu.window")).items([
            MenuItem::action(t!("action.minimize"), Minimize),
            MenuItem::action(t!("action.close_tab"), workspace::CloseTab),
            MenuItem::action(t!("action.close_window"), CloseWindow),
        ]),
    ]
}
