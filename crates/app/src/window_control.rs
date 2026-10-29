//! Debug-only agent driver for the real Rune window. Native events go through `AppKit`.
use crate::native::State;
use block2::RcBlock;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSWindow};
use objc2_foundation::NSOperationQueue;
use objc2_foundation::{NSPoint, NSSize, NSString};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError, mpsc},
    thread,
    time::Duration,
};
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
    Scroll { x: f64, y: f64, delta: i32 },
    Tree,
    Text { text: String },
    Snapshot { path: PathBuf },
    Resize { width: f64, height: f64 },
    Click { x: f64, y: f64 },
    Key { key: String },
    Theme { theme: String },
    Quit,
}

/// Where a request's answer goes: once, from the main thread or a screenshot worker.
#[derive(Debug)]
pub struct Reply(Mutex<Option<mpsc::Sender<Value>>>);

impl Reply {
    pub(crate) fn send(&self, value: Value) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(value);
        }
    }

    pub(crate) fn fail(&self, error: impl std::fmt::Display) {
        self.send(json!({ "error": error.to_string() }));
    }
}

/// Whether the agent control socket is enabled (`KOSSE_WINDOW_CONTROL=1`).
pub fn enabled() -> bool {
    std::env::var_os(ENABLE).is_some_and(|value| value == "1")
}

/// Starts listening when enabled. `deliver` hands a request to the window's event loop and
/// returns `false` once the loop is gone.
pub fn serve(deliver: impl Fn(Request, Reply) -> bool + Send + 'static) {
    if !enabled() {
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

#[expect(
    unsafe_code,
    reason = "AppKit copies the owned Send block and invokes it on the main queue; it outlives this call"
)]
pub fn dispatch(
    request: Request,
    reply: Reply,
    app: &rune::AppHandle<State>,
    tree: &std::sync::Arc<Mutex<Value>>,
) {
    if matches!(request, Request::Tree) {
        reply.send(tree.lock().unwrap_or_else(PoisonError::into_inner).clone());
        return;
    }
    if matches!(request, Request::Theme { .. } | Request::Quit) {
        app.update(move |s, tx| answer(request, reply, s, tx, &Value::Null));
        return;
    }
    // Run native input outside Rune's state-update callback. Dispatching an NSEvent there
    // re-enters the event loop while it holds its runtime, so edits can be overwritten.
    let pending = std::sync::Arc::new(Mutex::new(Some((request, reply))));
    let block = RcBlock::new(move || {
        if let Some((request, reply)) = pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            answer(
                request,
                reply,
                &mut State::default(),
                &mut rune::Tx::default(),
                &Value::Null,
            );
        }
    });
    unsafe {
        NSOperationQueue::mainQueue().addOperationWithBlock(&block);
    }
}

pub fn answer(request: Request, reply: Reply, state: &mut State, tx: &mut rune::Tx, tree: &Value) {
    if matches!(request, Request::Quit) {
        tx.quit();
        return;
    }
    let Some(window) = native_window() else {
        reply.fail("window unavailable");
        return;
    };
    match request {
        Request::Scroll { x, y, delta } => match scroll(&window, x, y, delta) {
            Ok(()) => reply.send(json!({"scrolled": delta})),
            Err(error) => reply.fail(error),
        },
        Request::Status => reply.send(status(&window)),
        Request::Tree => reply.send(tree.clone()),
        Request::Text { text } => {
            // Paste through the real native text input, not an alternate state mutation.
            for character in text.chars() {
                if let Err(error) = press(&window, &character.to_string()) {
                    reply.fail(error);
                    return;
                }
            }
            reply.send(json!({"typed": true}));
        }
        Request::Click { x, y } => match click(&window, x, y) {
            Ok(()) => reply.send(json!({"clicked": [x,y]})),
            Err(e) => reply.fail(e),
        },
        Request::Key { key } => match press(&window, &key) {
            Ok(()) => reply.send(json!({"pressed": key})),
            Err(e) => reply.fail(e),
        },
        Request::Resize { width, height } => {
            window.setContentSize(NSSize::new(width, height));
            reply.send(status(&window));
        }
        Request::Theme { theme } => {
            state.theme = match theme.as_str() {
                "dark" => Some(true),
                "light" => Some(false),
                "system" => None,
                _ => {
                    reply.fail("theme is light, dark or system");
                    return;
                }
            };
            reply.send(json!({"theme": theme}));
        }
        Request::Snapshot { path } => {
            if let Err(e) = writable_png(&path) {
                reply.fail(e);
                return;
            }
            let number = window.windowNumber();
            thread::spawn(move || {
                let result = capture(number, &path);
                match result {
                    Ok(()) => reply.send(json!({"path": path})),
                    Err(error) => reply.fail(error),
                }
            });
        }
        Request::Quit => {}
    }
}
fn native_window() -> Option<Retained<NSWindow>> {
    let marker = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(marker);
    app.keyWindow().or_else(|| app.windows().firstObject())
}
fn status(window: &NSWindow) -> Value {
    let size = window.contentView().map(|view| view.frame().size);
    json!({
        "renderer": "rune",
        "alpha": window.alphaValue(),
        "pid": std::process::id(),
        "windowNumber": window.windowNumber(),
        "title": window.title().to_string(),
        "width": size.map(|s| s.width),
        "height": size.map(|s| s.height),
        "visible": window.isVisible(),
        "focused": window.isKeyWindow(),
    })
}
fn click(window: &NSWindow, x: f64, y: f64) -> Result<(), String> {
    let height = window
        .contentView()
        .ok_or("no content view")?
        .frame()
        .size
        .height;
    // Window coordinates start at the bottom left; the Rune surface fills the content view.
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

/// A key press (down and up) to the window's first responder, the Rune surface.
fn press(window: &NSWindow, key: &str) -> Result<(), String> {
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

/// A real rendered window that does not cover or intercept the user's desktop.
pub fn conceal() {
    if let Some(window) = native_window() {
        window.setAlphaValue(0.01);
        window.setIgnoresMouseEvents(true);
        window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    }
}

fn scroll(window: &NSWindow, x: f64, y: f64, delta: i32) -> Result<(), String> {
    use objc2_core_graphics::{CGEvent, CGScrollEventUnit};
    let view = window.contentView().ok_or("no content view")?;
    let moved = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::MouseMoved,
        NSPoint::new(x, view.frame().size.height - y),
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        0,
        0,
        0.0,
    )
    .ok_or("could not create pointer event")?;
    view.mouseMoved(&moved);
    let wheel = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, delta, 0, 0)
        .ok_or("could not create wheel event")?;
    let event = NSEvent::eventWithCGEvent(&wheel).ok_or("could not create scroll event")?;
    view.scrollWheel(&event);
    Ok(())
}

/// `screencapture` writes into a fresh owner-only directory (`mkdir` fails rather than follow an
/// existing path), then the bytes go to `path` without following a symlink there.
fn capture(number: isize, path: &Path) -> Result<(), String> {
    static SHOTS_TAKEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let private = std::env::temp_dir().join(format!(
        "kosse-shot-{}-{}",
        std::process::id(),
        SHOTS_TAKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&private)
        .map_err(|e| e.to_string())?;
    let shot = private.join("shot.png");
    let taken = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", "-l", &number.to_string()])
        .arg(&shot)
        .output()
        .map_err(|e| e.to_string())
        .and_then(|output| {
            if output.status.success() {
                fs::read(&shot).map_err(|e| e.to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        });
    let _ = fs::remove_dir_all(&private);
    let bytes = taken?;
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::TRUNC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .map_err(|e| e.to_string())?;
    let mut file = fs::File::from(descriptor);
    file.write_all(&bytes).map_err(|e| e.to_string())
}
