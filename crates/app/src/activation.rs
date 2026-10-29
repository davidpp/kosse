//! Bringing the dashboard window's process to the front.
//!
//! Since macOS 14, activation is cooperative: an app may only become active when the active app
//! yields to it (or the user acts on it). The tray yields to the window process and asks it to
//! activate right after a menu click; the window process asks for activation itself when told to
//! show a route. When a terminal (`kosse`, `kosse open`) asked through the API, the active app is
//! the terminal, so macOS may leave the window behind it; that is a platform limit, recorded in
//! the human checklist.

use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSImage, NSRunningApplication};
use objc2_foundation::NSData;

/// The Dock and ⌘-Tab icon of the window process (512 px). A bare executable has no bundle icon,
/// so macOS would show its generic "exec" icon.
#[cfg(not(debug_assertions))]
const APP_ICON: &[u8] = include_bytes!("../../../brand/Kosse.iconset/icon_256x256@2x.png");
/// Debug builds: the preview tile (inverted, mirrored K), like the tray's preview icons.
#[cfg(debug_assertions)]
const APP_ICON: &[u8] = include_bytes!("../../../brand/Kosse-preview.iconset/icon_256x256@2x.png");

/// From the tray's main thread: let process `pid` come forward and ask it to.
pub fn bring_forward(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    let Some(window) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return;
    };
    if let Some(main_thread) = MainThreadMarker::new()
        && objc2::available!(macos = 14.0)
    {
        NSApplication::sharedApplication(main_thread).yieldActivationToApplication(&window);
    }
    let _ = window.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
}

/// From the window's main thread: ask for activation (granted when the tray yielded, or on
/// older macOS).
pub fn activate_self() {
    if let Some(main_thread) = MainThreadMarker::new() {
        if objc2::available!(macos = 14.0) {
            NSApplication::sharedApplication(main_thread).activate();
        } else {
            let _ = NSRunningApplication::currentApplication()
                .activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
    }
}

/// From the window's main thread: use Kossé's icon for the process instead of the generic one.
pub fn set_app_icon() {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(APP_ICON);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    #[expect(
        unsafe_code,
        reason = "setApplicationIconImage is unsafe only because it may not accept nil; a decoded image is passed"
    )]
    // SAFETY: a valid NSImage on the main thread.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}
