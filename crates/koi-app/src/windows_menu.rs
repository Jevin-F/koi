//! Tells macOS which menu of the menu bar is the window menu, so that it
//! lists the open windows there and in the Dock icon's menu.

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

/// Makes the menu titled `title` the window menu. Does nothing off the main
/// thread, where AppKit is unavailable, as in tests.
pub fn register(title: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let menu = app
        .mainMenu()
        .and_then(|bar| bar.itemWithTitle(&NSString::from_str(title)))
        .and_then(|item| item.submenu());
    if let Some(menu) = menu {
        app.setWindowsMenu(Some(&menu));
    }
}
