//! `kosse window <url>`: the dashboard in its own window, a `WKWebView` (wry) in a tao event loop.
//!
//! It runs as a child process of the app or of `kosse open`, so a `WebKit` crash or hang never
//! reaches the scanner or the tray, and closing the window frees the web engine's memory. It only
//! ever shows one loopback origin, `http://127.0.0.1:<port>`, under the same HTTP rules as a
//! browser tab. Links elsewhere (a service's own page) open in the default browser instead;
//! downloads, popups and anything that is not `http(s)` are refused. Website data is kept in
//! memory only (`incognito`), so the window writes nothing to disk.
//!
//! Protocol with the owner, one line each way:
//! - stdout: `ready` once the window exists, or `failed <reason>` before exiting with 1 (the
//!   owner reports it; the window writes nothing to stderr); `shown <url>` after each request.
//! - stdin: `<url>\t<title>` loads another dashboard address (same origin) and brings the window
//!   forward; end of input (the owner exited, even if it was killed) closes the window.

use std::{
    convert::Infallible,
    io::{self, BufRead, Write},
    process::ExitCode,
    thread,
};

use muda::{Menu, PredefinedMenuItem, Submenu};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::macos::{ActivationPolicy, EventLoopExtMacOS},
    window::WindowBuilder,
};
use wry::{NewWindowResponse, PageLoadEvent, WebViewBuilder};

use crate::{activation, cli::WindowArgs, opener};

const SIZE: LogicalSize<f64> = LogicalSize::new(1180.0, 780.0);
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(560.0, 420.0);

#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    #[error("not a dashboard URL (http://127.0.0.1:<port>/…): {0:?}")]
    NotDashboard(String),
    #[error("could not create the window: {0}")]
    Window(String),
    #[error("could not create the web view: {0}")]
    WebView(String),
    #[cfg(debug_assertions)]
    #[error("disabled by KOSSE_TEST_WINDOW=fail")]
    TestFailure,
}

enum Message {
    Navigate {
        url: String,
        title: String,
    },
    Loaded(String),
    StdinClosed,
    #[cfg(debug_assertions)]
    Dialog(String),
}

/// Runs the window until it closes, then exits the process. If it cannot start, it tells the
/// owner `failed <reason>` and returns the exit code.
pub fn run(args: &WindowArgs) -> ExitCode {
    match start(args) {
        Err(error) => {
            say(&format!("failed {error}"));
            ExitCode::FAILURE
        }
    }
}

fn start(args: &WindowArgs) -> Result<Infallible, WindowError> {
    let origin =
        Origin::of(&args.url).ok_or_else(|| WindowError::NotDashboard(args.url.clone()))?;
    let mode = TestMode::from_env()?;
    let mut event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    if !mode.visible() {
        // No Dock icon or activation either: a test run leaves the desktop alone.
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();
    // Without a menu bar, macOS routes no ⌘C/⌘V/⌘Q to the window.
    let menu = app_menu().map_err(|error| WindowError::Window(error.to_string()))?;
    menu.init_for_nsapp();
    let window = WindowBuilder::new()
        .with_title(&args.title)
        .with_inner_size(SIZE)
        .with_min_inner_size(MIN_SIZE)
        .with_visible(mode.visible())
        .build(&event_loop)
        .map_err(|error| WindowError::Window(error.to_string()))?;
    let navigation_origin = origin.clone();
    let loaded = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_url(&args.url)
        .with_incognito(true)
        // A click on the inactive window both activates it and reaches the page (macOS otherwise
        // swallows the first click, so a button seems to do nothing).
        .with_accept_first_mouse(true)
        .with_navigation_handler(move |url| {
            if navigation_origin.contains(&url) {
                return true;
            }
            open_elsewhere(&url);
            false
        })
        .with_new_window_req_handler(|url, _features| {
            open_elsewhere(&url);
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|_url, _path| false)
        .with_on_page_load_handler(move |event, url| {
            if matches!(event, PageLoadEvent::Finished) {
                let _ = loaded.send_event(Message::Loaded(url));
            }
        });
    #[cfg(debug_assertions)]
    let webview = if mode.reports_loads() {
        let reported = proxy.clone();
        webview.with_ipc_handler(move |request| {
            let _ = reported.send_event(Message::Dialog(request.body().clone()));
        })
    } else {
        webview
    };
    let webview = webview
        .build(&window)
        .map_err(|error| WindowError::WebView(error.to_string()))?;
    listen_to_owner(proxy, origin);
    if mode.visible() {
        activation::activate_self();
    }
    say("ready");

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        // Owned by the loop so the menu bar stays alive with the window.
        let _menu = &menu;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(Message::StdinClosed) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(Message::Navigate { url, title }) => {
                if webview.load_url(&url).is_err() {
                    report!("kosse: the window could not load {url}");
                }
                window.set_title(&title);
                if mode.visible() {
                    window.set_visible(true);
                    window.set_minimized(false);
                    window.set_focus();
                    activation::activate_self();
                }
                say(&format!("shown {url}"));
            }
            Event::UserEvent(Message::Loaded(url)) => {
                if mode.reports_loads() {
                    say(&format!("loaded {url}"));
                    #[cfg(debug_assertions)]
                    if webview.evaluate_script(test_probe::DIALOG).is_err() {
                        say("dialog unavailable");
                    }
                }
                if mode.exits_after_load() {
                    *control_flow = ControlFlow::Exit;
                }
            }
            #[cfg(debug_assertions)]
            Event::UserEvent(Message::Dialog(heading)) => say(&format!("dialog {heading}")),
            _ => {}
        }
    })
}

/// Debug builds only, headless test windows: after each load, report the heading of the
/// dashboard's open dialog (Inspect sheet, Stop confirmation) as `dialog "<heading>"` (JSON), or
/// `dialog null` when none opened within 5 s, so tests see what a deep link opened.
#[cfg(debug_assertions)]
mod test_probe {
    pub const DIALOG: &str = r"(() => {
  const deadline = Date.now() + 5000;
  const timer = setInterval(() => {
    const dialog = document.querySelector('dialog[open]');
    const heading = dialog && dialog.querySelector('h1, h2, h3');
    if (heading || Date.now() > deadline) {
      clearInterval(timer);
      window.ipc.postMessage(JSON.stringify(heading ? heading.textContent.trim() : null));
    }
  }, 100);
})();";
}

/// The standard application, edit and window menus.
fn app_menu() -> muda::Result<Menu> {
    let app = Submenu::with_items(
        "Kossé",
        true,
        &[
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    )?;
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )?;
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::close_window(None),
        ],
    )?;
    Menu::with_items(&[&app, &edit, &window])
}

/// Reads `<url>\t<title>` lines from the owner until its end of input.
fn listen_to_owner(proxy: tao::event_loop::EventLoopProxy<Message>, origin: Origin) {
    let reader = thread::Builder::new()
        .name("kosse-window-owner".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                let (url, title) = line.split_once('\t').unwrap_or((line.as_str(), "Kossé"));
                if !origin.contains(url) {
                    report!("kosse: the window ignores a URL outside its dashboard: {url:?}");
                    continue;
                }
                let navigate = Message::Navigate {
                    url: url.to_owned(),
                    title: title.to_owned(),
                };
                if proxy.send_event(navigate).is_err() {
                    return;
                }
            }
            let _ = proxy.send_event(Message::StdinClosed);
        });
    if reader.is_err() {
        // Without the reader the window cannot learn that its owner exited; it still closes
        // normally when the user closes it.
        report!("kosse: the window cannot follow its owner (no reader thread)");
    }
}

/// A link that leaves the dashboard: `http(s)` goes to the default browser, anything else nowhere.
fn open_elsewhere(url: &str) {
    if url.starts_with("http://") || url.starts_with("https://") {
        opener::open_or_report(url);
    }
}

/// One protocol line to the owner. A vanished owner is not an error for the window.
fn say(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

/// `http://127.0.0.1:<port>`, the only origin the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Origin(String);

impl Origin {
    fn of(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("http://127.0.0.1:")?;
        let (digits, _path) = rest.split_at(rest.find('/')?);
        let port: u16 = digits.parse().ok()?;
        // Canonical digits only, so the origin is a literal prefix of every URL on it.
        (port != 0 && port.to_string() == digits).then(|| Self(format!("http://127.0.0.1:{port}")))
    }

    /// Whether `url` is on this origin (its path, not just a prefix of the authority).
    fn contains(&self, url: &str) -> bool {
        url.strip_prefix(self.0.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// How tests run the window without showing it on the developer's desktop (debug builds only):
/// `KOSSE_TEST_WINDOW=headless` keeps it invisible and reports each finished page load as
/// `loaded <url>`; `headless-once` also closes it after the first load; `fail` makes it fail to
/// start, exercising the browser fallback.
#[derive(Clone, Copy)]
enum TestMode {
    Off,
    #[cfg(debug_assertions)]
    Headless {
        once: bool,
    },
}

impl TestMode {
    fn from_env() -> Result<Self, WindowError> {
        #[cfg(debug_assertions)]
        match std::env::var("KOSSE_TEST_WINDOW").as_deref() {
            Ok("headless") => return Ok(Self::Headless { once: false }),
            Ok("headless-once") => return Ok(Self::Headless { once: true }),
            Ok("fail") => return Err(WindowError::TestFailure),
            _ => {}
        }
        Ok(Self::Off)
    }

    fn visible(self) -> bool {
        matches!(self, Self::Off)
    }

    fn reports_loads(self) -> bool {
        !self.visible()
    }

    fn exits_after_load(self) -> bool {
        #[cfg(debug_assertions)]
        if let Self::Headless { once } = self {
            return once;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Origin;

    #[test]
    fn only_loopback_http_dashboard_urls_are_accepted() {
        let origin = Origin::of("http://127.0.0.1:47831/?inspect=1.2").expect("dashboard URL");
        assert_eq!(origin, Origin("http://127.0.0.1:47831".to_owned()));
        for url in [
            "http://localhost:47831/",
            "https://127.0.0.1:47831/",
            "http://127.0.0.1/",
            "http://127.0.0.1:0/",
            "http://127.0.0.1:47831",
            "http://127.0.0.1:47831@example.com/",
            "http://127.0.0.1:99999/",
            "file:///etc/passwd",
        ] {
            assert_eq!(Origin::of(url), None, "{url}");
        }
    }

    #[test]
    fn navigation_stays_on_the_exact_origin() {
        let origin = Origin::of("http://127.0.0.1:47831/").expect("dashboard URL");
        assert!(origin.contains("http://127.0.0.1:47831/"));
        assert!(origin.contains("http://127.0.0.1:47831/?stop=1.2"));
        assert!(!origin.contains("http://127.0.0.1:478310/"));
        assert!(!origin.contains("http://127.0.0.1:47831.example.com/"));
        assert!(!origin.contains("http://127.0.0.1:47831@example.com/"));
        assert!(!origin.contains("http://localhost:5173/"));
    }
}
