//! The primary application window and its lifecycle.
//!
//! Closing the window hides the app instead of destroying the window, so
//! running agents keep their state. Clicking the Dock icon brings it back.
//! Other windows, such as the settings, close normally.

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::*;
use koi_core::AppPaths;
use rust_i18n::t;

use crate::app_menu::CloseWindow;
use crate::workspace::WorkspaceView;
use crate::{APP_NAME, settings};

const DEFAULT_SIZE: Size<Pixels> = size(px(1540.), px(1120.));
const MIN_SIZE: Size<Pixels> = size(px(640.), px(400.));

/// Key context of windows other than the main one, where cmd-w closes the window.
pub const SECONDARY_CONTEXT: &str = "SecondaryWindow";

struct MainWindow {
    handle: AnyWindowHandle,
    view: Entity<WorkspaceView>,
}

impl Global for MainWindow {}

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "cmd-w",
        CloseWindow,
        Some(SECONDARY_CONTEXT),
    )]);
}

/// Opens the main window, centered on the primary display. `problems` found
/// during startup are shown in it.
pub fn open(paths: AppPaths, problems: Vec<String>, cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(DEFAULT_SIZE, cx)),
        window_min_size: Some(MIN_SIZE),
        titlebar: Some(TitlebarOptions {
            title: Some(APP_NAME.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (handle, view) = gpui_kit::open_window(options, cx, |window, cx| {
        window.on_window_should_close(cx, |_, cx| {
            cx.hide();
            false
        });
        settings::follow_system_appearance(window);
        cx.new(|cx| WorkspaceView::new(paths, problems, window, cx))
    })
    .expect("failed to open the main window");
    cx.set_global(MainWindow { handle, view });
}

/// Quits after confirming any unsaved editor changes.
pub fn quit(cx: &mut App) {
    let (handle, view) = {
        let main = cx.global::<MainWindow>();
        (main.handle, main.view.clone())
    };
    if !view.read(cx).has_unsaved_files(cx) {
        cx.quit();
        return;
    }
    handle
        .update(cx, |_, window, cx| {
            window.activate_window();
            window.open_alert_dialog(cx, |alert, _, _| {
                alert
                    .confirm()
                    .title(t!("files.quit_title"))
                    .description(t!("files.unsaved_quit"))
                    .ok_text(t!("action.quit", app = APP_NAME))
                    .ok_variant(ButtonVariant::Danger)
                    .on_ok(|_, _, cx| {
                        cx.quit();
                        true
                    })
            });
        })
        .ok();
}

/// Closes the active window; for the main window, hides the app instead.
pub fn close(cx: &mut App) {
    let main = cx.try_global::<MainWindow>().map(|window| window.handle);
    match cx.active_window() {
        Some(window) if Some(window) != main => {
            window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
        _ => cx.hide(),
    }
}

/// Brings the app and its window back to the front.
pub fn reopen(cx: &mut App) {
    cx.activate(true);
    for handle in cx.windows() {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .expect("window listed by the app is open");
    }
}
