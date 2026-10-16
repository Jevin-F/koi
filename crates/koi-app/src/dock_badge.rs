//! The count shown on the Dock icon.

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::runtime::Bool;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};

/// Asks macOS to allow the badge, which it hides otherwise, along with the
/// alerts and sounds of notifications. Only an app bundle can ask; without
/// one, as under `cargo run`, the notification center aborts the process.
pub fn request_permission() {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let ignore = RcBlock::new(|_: Bool, _: *mut NSError| {});
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Badge
                | UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound,
            &ignore,
        );
}

/// Shows `count` on the Dock icon, or clears it at zero. Does nothing off
/// the main thread, where AppKit is unavailable, as in tests.
pub fn show(count: usize) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let label = (count > 0).then(|| NSString::from_str(&count.to_string()));
    NSApplication::sharedApplication(mtm)
        .dockTile()
        .setBadgeLabel(label.as_deref());
}
