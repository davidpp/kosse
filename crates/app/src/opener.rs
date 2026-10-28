//! Opening a URL in the user's default browser (`/usr/bin/open`): service links from the tray and
//! the window, `serve` without `--no-open`, and the fallback when the dashboard window cannot
//! start. Only `http(s)` URLs are passed on, as one argument, never through a shell. `open` is
//! spawned and reaped on a short-lived thread, so no caller (the `AppKit` main threads included)
//! ever waits for it.

use std::{process::Command, thread};

const OPEN: &str = "/usr/bin/open";

/// Starts opening `url`; returns why it could not even start. A later failure is reported.
pub fn open(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("refusing to open a non-HTTP URL: {url:?}"));
    }
    let mut child = Command::new(program())
        .arg(url)
        .spawn()
        .map_err(|error| format!("could not open {url} in a browser: {error}"))?;
    let url = url.to_owned();
    let reaper = thread::Builder::new()
        .name("kosse-opener".into())
        .spawn(move || match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => report!("kosse: could not open {url} in a browser ({status})"),
            Err(error) => report!("kosse: could not open {url} in a browser: {error}"),
        });
    if reaper.is_err() {
        // Without the thread the child is reaped at exit; nothing else depends on it.
        report!("kosse: opening a browser without waiting for it");
    }
    Ok(())
}

/// `open` and report a failure; the caller has nothing better to do with it.
pub fn open_or_report(url: &str) {
    if let Err(reason) = open(url) {
        report!("kosse: {reason}");
    }
}

/// Debug builds only: `KOSSE_TEST_OPENER` replaces `/usr/bin/open`, so tests check which URL the
/// browser would get without opening one on the developer's desktop.
fn program() -> String {
    #[cfg(debug_assertions)]
    if let Some(program) = std::env::var_os("KOSSE_TEST_OPENER") {
        return program.to_string_lossy().into_owned();
    }
    OPEN.to_owned()
}
