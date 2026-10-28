//! Bringing the dashboard window's process to the front.
//!
//! Since macOS 14, activation is cooperative: an app may only become active when the active app
//! yields to it (or the user acts on it). The tray yields to the window process and asks it to
//! activate right after a menu click; the window process asks for activation itself when told to
//! show a route. When a terminal (`kosse`, `kosse open`) asked through the API, the active app is
//! the terminal, so macOS may leave the window behind it; that is a platform limit, recorded in
//! the human checklist.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};

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
