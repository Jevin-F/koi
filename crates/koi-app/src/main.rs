//! Koi: one window to run and watch many CLI coding agents.

use koi_core::{AppPaths, integration};
use rust_i18n::t;

mod app_menu;
mod dock_badge;
mod fs_watcher;
mod i18n;
mod main_window;
mod picker;
mod settings;
mod settings_view;
mod shortcuts;
mod terminal;
mod workspace;

rust_i18n::i18n!("locales", fallback = "en");

/// User-facing application name.
const APP_NAME: &str = "Koi";
/// Must match `CFBundleIdentifier` in `packaging/macos/Info.plist`, or macOS
/// drops the app's notifications.
const BUNDLE_ID: &str = "dev.koi.Koi";

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let recorded = match args.first().map(String::as_str) {
        Some(integration::RECORD_PROMPT_ARG) => Some(integration::record_claude_prompt()),
        Some(integration::RECORD_TURN_ARG) => Some(integration::record_codex_turn(
            args.get(1).map_or("", String::as_str),
        )),
        _ => None,
    };
    if let Some(result) = recorded {
        if let Err(error) = result {
            eprintln!("Could not record the agent's session: {error}");
        }
        return;
    }
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.on_reopen(main_window::reopen);
    app.run(|cx| {
        cx.set_app_identity(BUNDLE_ID, APP_NAME);
        dock_badge::request_permission();
        gpui_kit::init(cx);
        app_menu::init(cx);
        terminal::init(cx);
        workspace::init(cx);
        main_window::init(cx);
        let home = std::env::home_dir().expect("the home directory is known");
        let paths = AppPaths::in_home(&home);
        let problems: Vec<String> = settings::init(&paths, cx)
            .map(|error| t!("error.settings", error = error).into_owned())
            .into_iter()
            .collect();
        main_window::open(paths, problems, cx);
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::BUNDLE_ID;
    use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext, point, px};

    #[gpui_kit::test]
    fn starts_and_renders_a_restored_session(cx: &mut TestAppContext) {
        let home = std::env::temp_dir().join(format!("koi-startup-test-{}", std::process::id()));
        // A plain folder beside Koi's state, so that saving state does not
        // show up as file changes in the project.
        let project = home.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let paths = koi_core::AppPaths::in_home(&home);
        let mut workspace = koi_core::Workspace::default();
        workspace.add_project(project.clone());
        workspace.add_session(&project, project.clone(), "shell", Vec::new());
        koi_core::store::save(&paths.state_file(), &workspace).unwrap();
        let state_file = paths.state_file();
        let mut settings = koi_core::settings::Settings::default();
        settings.appearance = koi_core::settings::Appearance::Light;
        koi_core::store::save(&paths.settings_file(), &settings).unwrap();
        cx.update(|cx| {
            cx.set_app_identity(BUNDLE_ID, super::APP_NAME);
            gpui_kit::init(cx);
            super::app_menu::init(cx);
            super::terminal::init(cx);
            super::workspace::init(cx);
            super::main_window::init(cx);
            super::settings::init(&paths, cx);
            super::main_window::open(paths, Vec::new(), cx);
        });
        let window = cx.update(|cx| cx.windows()[0]);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            // A plain folder starts agents but has no worktrees or changes.
            assert!(window.try_find(("start-agent", 1usize)).is_some());
            assert!(window.try_find(("new-worktree", 0usize)).is_none());
            assert!(window.try_find("show-changes").is_none());
            let sidebar = window.find("sidebar-rows").bounds();
            let from = point(sidebar.right() - px(2.), sidebar.center().y);
            window.drag(from, point(from.x + px(80.), from.y), cx);
            window.render_frame(cx);
            assert!(window.find("sidebar-rows").bounds().size.width > sidebar.size.width + px(40.));
            window.click(("rename-session", 2usize), cx);
            window.render_frame(cx);
            assert!(window.try_find("dialog").is_some());
            assert!(window.try_find("use-agent-name").is_some());
            window.click("session-name-input", cx);
            window.input("Review PR 385", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("session-name-input").value(),
                Some("Review PR 385")
            );
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.press("enter", cx);
            window.render_frame(cx);
            let hover_color = cx.theme().foreground.opacity(0.10);
            let selected_color = cx.theme().foreground.opacity(0.20);
            let selected_hover_color = cx.theme().foreground.opacity(0.28);
            assert_ne!(hover_color, selected_color);
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.background.as_solid() == Some(selected_color))
            );
            assert!(
                !window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.background.as_solid() == Some(hover_color))
            );
            window.hover("show-terminal", cx);
            window.render_frame(cx);
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.background.as_solid() == Some(selected_hover_color))
            );
            window.hover("show-files", cx);
            window.render_frame(cx);
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.background.as_solid() == Some(hover_color))
            );
            for mode in ["show-terminal", "show-files"] {
                window.hover(mode, cx);
                window.render_frame(cx);
                window.click(mode, cx);
                window.render_frame(cx);
            }
            Theme::change(ThemeMode::Dark, None, cx);
            cx.refresh_windows();
            window.hover("sidebar-add-project", cx);
            window.render_frame(cx);
            assert!(window.painted_quads().iter().any(
                |quad| quad.background.as_solid() == Some(cx.theme().foreground.opacity(0.20))
            ));
            window.hover("show-terminal", cx);
            window.render_frame(cx);
            assert!(window.painted_quads().iter().any(
                |quad| quad.background.as_solid() == Some(cx.theme().foreground.opacity(0.10))
            ));
            window.hover("status-settings", cx);
            window.render_frame(cx);
            assert!(window.painted_quads().iter().any(
                |quad| quad.background.as_solid() == Some(cx.theme().foreground.opacity(0.12))
            ));
            window.click("status-settings", cx);
        })
        .unwrap();
        assert_eq!(cx.update(|cx| cx.windows().len()), 2);
        let saved: koi_core::Workspace = koi_core::store::load(&state_file).unwrap();
        assert_eq!(saved.sessions[0].label.as_deref(), Some("Review PR 385"));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bundle_id_matches_info_plist() {
        let plist = include_str!("../../../packaging/macos/Info.plist");
        let expected = format!("<key>CFBundleIdentifier</key>\n    <string>{BUNDLE_ID}</string>");
        assert!(plist.contains(&expected));
    }
}
