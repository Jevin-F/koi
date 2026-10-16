//! The user's preferences: loaded at startup, changed in the settings
//! window, saved to `settings.json` and applied at once.

use std::io;
use std::path::PathBuf;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use koi_core::settings::{Appearance, Settings};
use koi_core::{AppPaths, store};

use crate::{app_menu, i18n, shortcuts};

struct AppSettings {
    settings: Settings,
    path: PathBuf,
}

impl Global for AppSettings {}

/// Loads and applies the saved settings. An unreadable file is reported and
/// the defaults are used; it is replaced on the next change.
pub fn init(paths: &AppPaths, cx: &mut App) -> Option<io::Error> {
    let path = paths.settings_file();
    let (settings, problem) = match store::load(&path) {
        Ok(settings) => (settings, None),
        Err(error) => (Settings::default(), Some(error)),
    };
    cx.set_global(AppSettings { settings, path });
    apply(cx);
    problem
}

/// The settings in effect.
pub fn get(cx: &App) -> &Settings {
    &cx.global::<AppSettings>().settings
}

/// Changes the settings, then saves and applies them.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Settings)) -> io::Result<()> {
    let app_settings = cx.global_mut::<AppSettings>();
    change(&mut app_settings.settings);
    let saved = store::save(&app_settings.path, &app_settings.settings);
    apply(cx);
    saved
}

/// The font of terminals and code.
#[derive(Clone)]
pub struct CodeFont {
    pub family: SharedString,
    pub size: Pixels,
}

/// The chosen font of terminals and code.
pub fn code_font(cx: &App) -> CodeFont {
    let settings = get(cx);
    CodeFont {
        family: settings.font_family.clone().into(),
        size: px(settings.font_size()),
    }
}

fn apply(cx: &mut App) {
    let settings = get(cx).clone();
    i18n::set_language(settings.language);
    match settings.appearance {
        Appearance::Dark => Theme::change(ThemeMode::Dark, None, cx),
        Appearance::Light => Theme::change(ThemeMode::Light, None, cx),
        Appearance::System => Theme::sync_system_appearance(None, cx),
    }
    shortcuts::bind(&settings.shortcuts, cx);
    // Menus show shortcuts and translated names, so they are rebuilt.
    app_menu::set_menus(cx);
    cx.refresh_windows();
}

/// Keeps a window's theme in step with macOS when following the system.
pub fn follow_system_appearance(window: &mut Window) {
    window
        .observe_window_appearance(|window, cx| {
            if get(cx).appearance == Appearance::System {
                Theme::sync_system_appearance(Some(window), cx);
            }
        })
        .detach();
}
