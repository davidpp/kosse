//! Debug builds only: the dashboard window's agent control socket (docs/agent-drive.md), so an
//! agent can read and drive the real `WKWebView` window rather than a stand-in browser.
//!
//! It listens only when the window process has `KOSSE_WINDOW_CONTROL=1` and
//! `KOSSE_WINDOW_CONTROL_SOCKET=<path>` (set by `bun run app`); release builds contain none of
//! this. The socket is owner-only (0600) in an owner-only directory, and removed when the window
//! closes. One request per connection: a JSON line in (at most 1 MiB, read within 5 s), a JSON
//! line out (`{"error": …}` on failure). Snapshots are written only as new or regular `.png`
//! files (never through a symlink) in `KOSSE_WINDOW_CONTROL_DIR` or the temporary directory.
//! - `{"op":"status"}`: URL, title, logical size, visibility, focus, the process's app name;
//! - `{"op":"eval","js":…}`: evaluates `js` in the page and returns its JSON value (a promise is
//!   not awaited; `scripts/app.ts` wraps the code to make that work);
//! - `{"op":"snapshot","path":…}`: writes the visible page as a PNG (`WKWebView`'s own snapshot,
//!   no screen-recording permission);
//! - `{"op":"resize","width":…,"height":…}`: sets the window's content size (points);
//! - `{"op":"click","x":…,"y":…}`: a real left click (`NSEvent` mouse down and up through the
//!   window) at page coordinates, so hit-testing, focus and the first click are `AppKit`'s own;
//! - `{"op":"key","key":…}`: a real key press (`j`, `k`, `/`, `Enter`, `Escape`, `Tab`, `Backspace`,
//!   `ArrowDown`, `ArrowUp`, or one printable character) to the focused element;
//! - `{"op":"theme","theme":"light"|"dark"|"system"}`: the window's appearance, which the page
//!   sees as `prefers-color-scheme`;
//! - `{"op":"quit"}`: closes the window, as ⌘W does (answered before it closes).
//!
//! Requests reach the window's main thread as events; the reply comes back on a channel.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError, mpsc},
    thread,
    time::Duration,
};

use block2::RcBlock;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSRunningApplication};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSDictionary, NSError};
use objc2_foundation::{NSPoint, NSString};
use serde::Deserialize;
use serde_json::{Value, json};
use tao::{
    dpi::LogicalSize,
    window::{Theme, Window},
};
use wry::{WebView, WebViewExtMacOS};

const ENABLE: &str = "KOSSE_WINDOW_CONTROL";
const SOCKET: &str = "KOSSE_WINDOW_CONTROL_SOCKET";
const SHOTS: &str = "KOSSE_WINDOW_CONTROL_DIR";
const ANSWER_WITHIN: Duration = Duration::from_secs(10);
const IO_WITHIN: Duration = Duration::from_secs(5);
const MAX_REQUEST: u64 = 1024 * 1024;
/// Consecutive accept failures (say `EMFILE`) before the socket gives up.
const MAX_ACCEPT_FAILURES: u32 = 50;

/// The bound socket's path, removed when the window closes.
static BOUND: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Status,
    Eval { js: String },
    Snapshot { path: PathBuf },
    Resize { width: f64, height: f64 },
    Click { x: f64, y: f64 },
    Key { key: String },
    Theme { theme: String },
    Quit,
}

/// Where a request's answer goes: once, from the main thread or a `WebKit` callback.
#[derive(Debug)]
pub struct Reply(Mutex<Option<mpsc::Sender<Value>>>);

impl Reply {
    fn send(&self, value: Value) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(value);
        }
    }

    fn fail(&self, error: impl std::fmt::Display) {
        self.send(json!({ "error": error.to_string() }));
    }
}

/// Starts listening when enabled. `deliver` hands a request to the window's event loop and
/// returns `false` once the loop is gone.
pub fn serve(deliver: impl Fn(Request, Reply) -> bool + Send + 'static) {
    if std::env::var_os(ENABLE).is_none_or(|value| value != "1") {
        return;
    }
    let Some(path) = std::env::var_os(SOCKET).map(PathBuf::from) else {
        report!("kosse: {ENABLE}=1 needs {SOCKET}=<path>; not listening");
        return;
    };
    let listener = match bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            report!(
                "kosse: window control socket unavailable at {}: {error}",
                path.display()
            );
            return;
        }
    };
    report!(
        "kosse: window control socket listening at {}",
        path.display()
    );
    let _ = BOUND.set(path);
    let spawned = thread::Builder::new()
        .name("kosse-window-control".into())
        .spawn(move || {
            let mut failures = 0;
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        failures = 0;
                        if !handle(&stream, &deliver) {
                            return;
                        }
                    }
                    Err(error) => {
                        failures += 1;
                        if failures >= MAX_ACCEPT_FAILURES {
                            report!("kosse: window control socket stops after: {error}");
                            return;
                        }
                        thread::sleep(Duration::from_millis(20 * u64::from(failures)));
                    }
                }
            }
        });
    if spawned.is_err() {
        report!("kosse: window control socket has no thread; not listening");
    }
}

fn bind(path: &Path) -> std::io::Result<UnixListener> {
    // A socket left by a previous window refuses the bind; remove only a socket.
    if fs::symlink_metadata(path)
        .is_ok_and(|meta| std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()))
    {
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// `KOSSE_TEST_WINDOW=transparent`: nothing to see and nothing to click for the person at the
/// desktop, while the window still counts as shown for `WebKit` and `AppKit`.
pub fn make_transparent(webview: &WebView) {
    let window = webview.ns_window();
    window.setAlphaValue(0.01);
    window.setIgnoresMouseEvents(true);
    // Above other apps' windows, so macOS does not count it occluded (WebKit would then throttle
    // the page as a hidden one); being transparent and click-through, it covers nothing.
    window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
}

/// Removes the socket when the window closes (debug builds; nothing else lives at that path).
pub fn unlink() {
    if let Some(path) = BOUND.get() {
        let _ = fs::remove_file(path);
    }
}

/// Answers one connection; `false` when the event loop is gone.
fn handle(stream: &UnixStream, deliver: &impl Fn(Request, Reply) -> bool) -> bool {
    let _ = stream.set_read_timeout(Some(IO_WITHIN));
    let _ = stream.set_write_timeout(Some(IO_WITHIN));
    let mut line = String::new();
    let mut alive = true;
    let reply = match BufReader::new(std::io::Read::take(stream, MAX_REQUEST)).read_line(&mut line)
    {
        Err(error) => json!({ "error": error.to_string() }),
        Ok(_) => match serde_json::from_str::<Request>(&line) {
            Err(error) => json!({ "error": format!("unknown request: {error}") }),
            // Answered before the window closes, so the reply never races the exit.
            Ok(Request::Quit) => {
                let _ = writeln!(&mut &*stream, "{}", json!({ "closing": true }));
                let (sender, _received) = mpsc::channel();
                let _ = deliver(Request::Quit, Reply(Mutex::new(Some(sender))));
                return false;
            }
            Ok(request) => {
                let (sender, received) = mpsc::channel();
                if deliver(request, Reply(Mutex::new(Some(sender)))) {
                    received.recv_timeout(ANSWER_WITHIN).unwrap_or_else(
                        |_| json!({ "error": "the window did not answer within 10 s" }),
                    )
                } else {
                    alive = false;
                    json!({ "error": "the window is closing" })
                }
            }
        },
    };
    let _ = writeln!(&mut &*stream, "{reply}");
    alive
}

/// Runs `request` on the main thread. Returns `true` when the window should close.
pub fn answer(request: Request, reply: Reply, window: &Window, webview: &WebView) -> bool {
    match request {
        Request::Status => reply.send(status(window, webview)),
        Request::Eval { js } => {
            let shared = std::sync::Arc::new(reply);
            let callback = std::sync::Arc::clone(&shared);
            let evaluated = webview.evaluate_script_with_callback(&js, move |result| {
                // The value arrives as JSON text; anything else is passed on as a string.
                let value = serde_json::from_str(&result).unwrap_or(Value::String(result));
                callback.send(json!({ "value": value }));
            });
            if let Err(error) = evaluated {
                shared.fail(error);
            }
        }
        Request::Snapshot { path } => match writable_png(&path) {
            Ok(()) => snapshot(webview, path, reply),
            Err(error) => reply.fail(error),
        },
        Request::Resize { width, height } => {
            window.set_inner_size(LogicalSize::new(width, height));
            reply.send(status(window, webview));
        }
        Request::Click { x, y } => match click(webview, x, y) {
            Ok(()) => reply.send(json!({ "clicked": [x, y] })),
            Err(error) => reply.fail(error),
        },
        Request::Key { key } => match press(webview, &key) {
            Ok(()) => reply.send(json!({ "pressed": key })),
            Err(error) => reply.fail(error),
        },
        Request::Theme { theme } => {
            let theme = match theme.as_str() {
                "light" => Some(Theme::Light),
                "dark" => Some(Theme::Dark),
                "system" => None,
                _ => {
                    reply.fail("theme is light, dark or system");
                    return false;
                }
            };
            window.set_theme(theme);
            reply.send(status(window, webview));
        }
        Request::Quit => {
            reply.send(json!({ "closing": true }));
            return true;
        }
    }
    false
}

fn status(window: &Window, webview: &WebView) -> Value {
    let scale = window.scale_factor();
    let size: LogicalSize<f64> = window.inner_size().to_logical(scale);
    let app = NSRunningApplication::currentApplication();
    let position = window
        .outer_position()
        .ok()
        .map(|position| position.to_logical::<f64>(scale))
        .map(|position| [position.x, position.y]);
    json!({
        "pid": std::process::id(),
        "position": position,
        "url": webview.url().ok(),
        "title": window.title(),
        "width": size.width,
        "height": size.height,
        "scale": scale,
        "visible": window.is_visible(),
        "focused": window.is_focused(),
        "minimized": window.is_minimized(),
        "alpha": webview.ns_window().alphaValue(),
        "occluded": !webview
            .ns_window()
            .occlusionState()
            .contains(objc2_app_kit::NSWindowOcclusionState::Visible),
        "appName": app.localizedName().map(|name| name.to_string()),
        "bundleIdentifier": app.bundleIdentifier().map(|id| id.to_string()),
    })
}

/// `WKWebView.takeSnapshot`: the visible page at the window's scale, written as a PNG.
fn snapshot(webview: &WebView, path: PathBuf, reply: Reply) {
    let handler = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        #[expect(
            unsafe_code,
            reason = "WebKit's completion handler passes raw pointers to an image or an error"
        )]
        // SAFETY: WebKit passes either an image or an error, each valid for this call.
        let (image, error) = unsafe { (image.as_ref(), error.as_ref()) };
        let written = match (image, error) {
            (Some(image), _) => png(image).and_then(|bytes| {
                write_png(&path, &bytes).map_err(|error| format!("{}: {error}", path.display()))
            }),
            (None, Some(error)) => Err(error.localizedDescription().to_string()),
            (None, None) => Err("WebKit returned no snapshot".to_owned()),
        };
        match written {
            Ok(()) => reply.send(json!({ "path": path })),
            Err(error) => reply.fail(error),
        }
    });
    let view = webview.webview();
    #[expect(
        unsafe_code,
        reason = "takeSnapshot is unsafe only for its block's signature, which matches WebKit's"
    )]
    // SAFETY: on the main thread, with a handler of the declared type; no configuration means the
    // visible bounds.
    unsafe {
        view.takeSnapshotWithConfiguration_completionHandler(None, &handler);
    }
}

fn png(image: &NSImage) -> Result<Vec<u8>, String> {
    let tiff = image
        .TIFFRepresentation()
        .ok_or("the snapshot has no image data")?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("unreadable snapshot")?;
    #[expect(
        unsafe_code,
        reason = "representationUsingType is unsafe only for the dictionary's generic types"
    )]
    // SAFETY: an empty properties dictionary has the expected key and value types.
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("the snapshot could not be encoded as PNG")?;
    Ok(data.to_vec())
}

/// A left click at page coordinates (CSS pixels from the top left, the web view's points).
fn click(webview: &WebView, x: f64, y: f64) -> Result<(), String> {
    let window = webview.ns_window();
    let height = webview.webview().frame().size.height;
    // Window coordinates start at the bottom left; the web view fills the content view.
    let location = NSPoint::new(x, height - y);
    let number = window.windowNumber();
    for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            location,
            NSEventModifierFlags::empty(),
            0.0,
            number,
            None,
            0,
            1,
            1.0,
        )
        .ok_or("could not create the mouse event")?;
        window.sendEvent(&event);
    }
    Ok(())
}

/// A key press (down and up) to the window's first responder, the web view.
fn press(webview: &WebView, key: &str) -> Result<(), String> {
    let (characters, code) = match key {
        "Enter" => ("\r".to_owned(), 36),
        "Escape" => ("\u{1b}".to_owned(), 53),
        "Tab" => ("\t".to_owned(), 48),
        "Backspace" => ("\u{7f}".to_owned(), 51),
        "ArrowDown" => ("\u{f701}".to_owned(), 125),
        "ArrowUp" => ("\u{f700}".to_owned(), 126),
        "j" => ("j".to_owned(), 38),
        "k" => ("k".to_owned(), 40),
        "/" => ("/".to_owned(), 44),
        other if other.chars().count() == 1 => (other.to_owned(), 0),
        other => return Err(format!("unsupported key {other:?}")),
    };
    let window = webview.ns_window();
    let characters = NSString::from_str(&characters);
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            window.windowNumber(),
            None,
            &characters,
            &characters,
            false,
            code,
        )
        .ok_or("could not create the key event")?;
        window.sendEvent(&event);
    }
    Ok(())
}

/// A snapshot goes only to an absolute `.png` path in `KOSSE_WINDOW_CONTROL_DIR` or the temporary
/// directory, as a new file or over a regular file (never a symlink, socket or directory).
fn writable_png(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.extension().is_none_or(|extension| extension != "png") {
        return Err("the snapshot path must be an absolute .png path".to_owned());
    }
    if let Ok(meta) = fs::symlink_metadata(path)
        && !meta.file_type().is_file()
    {
        return Err(format!(
            "{} exists and is not a regular file",
            path.display()
        ));
    }
    let parent = path
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .ok_or_else(|| format!("{}: no such directory", path.display()))?;
    let allowed = [
        std::env::var_os(SHOTS).map(PathBuf::from),
        Some(std::env::temp_dir()),
    ];
    let inside = allowed
        .iter()
        .flatten()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| parent.starts_with(root));
    if inside {
        Ok(())
    } else {
        Err(format!(
            "snapshots go under ${SHOTS} or the temporary directory, not {}",
            parent.display()
        ))
    }
}

/// Writes without following a symlink planted since the check.
fn write_png(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use rustix::fs::{Mode, OFlags};
    let fd = rustix::fs::open(
        path,
        OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    std::io::Write::write_all(&mut fs::File::from(fd), bytes)
}
