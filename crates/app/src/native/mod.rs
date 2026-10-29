//! Rune dashboard. The window reads the owner's API; it never scans or signals.
mod client;
mod customize;
mod view;

use crate::{
    cli::WindowArgs,
    route::{Route, Show},
};
use kosse_core::{
    action::{ActionRequest, ActionResult},
    model::{ProcessIdentity, Service, Snapshot},
};
use rune::{App, AppSpec, TextEdit};
use std::{
    io::{self, BufRead, Write},
    net::{Ipv4Addr, SocketAddr},
    process::ExitCode,
    thread,
};
use tokio::sync::mpsc;

/// How long a service that stopped listening stays listed as ended.
const ENDED_RETENTION_MS: u64 = 60_000;

pub struct State {
    pub(crate) theme: Option<bool>,
    snapshot: Option<Snapshot>,
    search: TextEdit,
    editor: Option<customize::Editor>,
    orphan_edit: TextEdit,
    all: bool,
    notes: bool,
    selected: Option<ProcessIdentity>,
    confirmation: Option<Confirmation>,
    error: Option<String>,
    connected: bool,
    connection_error: Option<String>,
    ended: Vec<(Service, u64)>,
    requests: Option<mpsc::UnboundedSender<client::Command>>,
    /// A route from the owner waiting for a snapshot that can resolve it.
    route: Option<Route>,
    /// The instance's base URL, `http://127.0.0.1:<port>/`.
    base_url: String,
    current: Route,
    /// Counts routes received; 0 until the owner's first one.
    route_generation: u64,
}

struct Confirmation {
    service: Service,
    request: ActionRequest,
    result: Option<ActionResult>,
    busy: bool,
    force: bool,
    error: Option<String>,
}

impl Confirmation {
    /// A request in flight or an outcome not yet acknowledged: never replaced by a route.
    fn pinned(&self) -> bool {
        self.busy || self.result.is_some()
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            theme: None,
            snapshot: None,
            search: TextEdit::new(""),
            editor: None,
            orphan_edit: TextEdit::new(""),
            all: false,
            notes: false,
            selected: None,
            confirmation: None,
            error: None,
            connected: false,
            connection_error: None,
            ended: Vec::new(),
            requests: None,
            route: None,
            base_url: String::new(),
            current: Route::Dashboard,
            route_generation: 0,
        }
    }
}

impl State {
    fn accept(&mut self, snapshot: Snapshot) {
        if let Some(next) = &snapshot.observation {
            self.ended.retain(|(service, seen)| {
                next.scanned_at_ms.saturating_sub(*seen) < ENDED_RETENTION_MS
                    && !next
                        .services
                        .iter()
                        .any(|s| s.process.identity == service.process.identity)
            });
            if let Some(previous) = self.snapshot.as_ref().and_then(|s| s.observation.as_ref()) {
                for service in &previous.services {
                    let identity = service.process.identity;
                    if !next.services.iter().any(|s| s.process.identity == identity)
                        && !self
                            .ended
                            .iter()
                            .any(|(s, _)| s.process.identity == identity)
                    {
                        self.ended.push((service.clone(), previous.scanned_at_ms));
                    }
                }
            }
        }
        self.snapshot = Some(snapshot);
        self.apply_route();
    }

    fn send(&mut self, command: client::Command) {
        if self
            .requests
            .as_ref()
            .is_none_or(|sender| sender.send(command).is_err())
        {
            self.error = Some("Connection lost. Reopen Kossé to reconnect.".into());
        }
    }

    fn confirm(&mut self, service: Service) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        self.confirmation = Some(Confirmation {
            request: ActionRequest {
                instance: snapshot.instance.clone(),
                identity: service.process.identity,
                ports: service.endpoints.iter().map(|e| e.port).collect(),
            },
            service,
            result: None,
            busy: false,
            force: false,
            error: None,
        });
    }

    fn navigate(&mut self, route: Route) {
        self.current = route;
        self.route_generation += 1;
        self.route = Some(route);
        self.apply_route();
    }

    fn confirmation_pinned(&self) -> bool {
        self.confirmation.as_ref().is_some_and(Confirmation::pinned)
    }

    fn editor_pinned(&self) -> bool {
        self.editor.as_ref().is_some_and(customize::Editor::pinned)
    }

    /// Applies the pending route once the snapshot can resolve it. A stop in flight (or its
    /// unacknowledged outcome) and unsaved customization are never discarded by a route.
    fn apply_route(&mut self) {
        let Some(route) = self.route else {
            return;
        };
        let target = match route {
            Route::Dashboard => {
                self.route = None;
                self.selected = None;
                if !self.confirmation_pinned() {
                    self.confirmation = None;
                }
                return;
            }
            Route::Inspect(target) | Route::Stop(target) => target,
        };
        let Some(observation) = self.snapshot.as_ref().and_then(|s| s.observation.as_ref()) else {
            return;
        };
        self.route = None;
        let identity = target.identity();
        let Some(service) = observation
            .services
            .iter()
            .find(|s| s.process.identity == identity)
            .cloned()
        else {
            self.error = Some("This process is no longer in the current observation.".into());
            return;
        };
        self.selected = Some(identity);
        if !self.editor_pinned() {
            self.editor = None;
        }
        if self.confirmation_pinned() {
            if matches!(route, Route::Stop(_)) {
                self.error =
                    Some("Finish the current stop before stopping another process.".into());
            }
            return;
        }
        self.confirmation = None;
        if matches!(route, Route::Stop(_)) {
            if self.editor.is_some() {
                self.error =
                    Some("Save or cancel the customization before stopping a process.".into());
            } else {
                self.confirm(service);
            }
        }
    }
}

pub fn run(args: &WindowArgs) -> ExitCode {
    let Some(port) = args
        .url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|s| s.split_once('/').map(|(port, _)| port))
        .and_then(|s| s.parse::<u16>().ok().filter(|port| *port > 0))
    else {
        say("failed not a Kossé loopback URL");
        return ExitCode::FAILURE;
    };
    #[cfg(debug_assertions)]
    if std::env::var("KOSSE_TEST_WINDOW").as_deref() == Ok("fail") {
        say("failed disabled by KOSSE_TEST_WINDOW=fail");
        return ExitCode::FAILURE;
    }
    let runtime = match crate::runtime() {
        Ok(rt) => rt,
        Err(error) => {
            say(&format!("failed {error}"));
            return ExitCode::FAILURE;
        }
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let (sender, receiver) = mpsc::unbounded_channel();
    let base_url = format!("http://{addr}/");
    let title = args.title.clone();
    let test_mode = test_mode();
    #[cfg(debug_assertions)]
    let once = test_mode == "headless-once";
    let hidden = !test_mode.is_empty();
    let spec = AppSpec {
        min_size: Some((640.0, 440.0)),
        on_key: Some(view::on_key),
        fonts: &[
            include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
            include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
            include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
            include_bytes!("../../../../brand/fonts/KibernetykMono-SemiBold.ttf"),
        ],
        ..AppSpec::new("Kossé", (1180.0, 780.0), State::default, view::view)
    };
    let app = App::new(spec)
        .args(Vec::<String>::new())
        .standard_menus()
        .tab_visits_controls(true)
        .init(move || State {
            requests: Some(sender),
            base_url: base_url.clone(),
            ..State::default()
        })
        .on_window(move |_| {
            #[cfg(debug_assertions)]
            if hidden {
                crate::window_control::conceal();
            }
        })
        .on_ready(move |_| {
            if !hidden {
                crate::activation::set_app_icon();
                crate::activation::activate_self();
            }
            say("ready");
        });
    let app = if hidden { app.accessory() } else { app };
    #[cfg(debug_assertions)]
    let app = instrument(app, hidden, once);
    let handle = app.handle();
    handle.update(move |_, tx| {
        tx.set_title(title);
    });
    let background = runtime.spawn(client::run(addr, receiver, handle.clone()));
    // The owner's routes, one JSON `Show` per line; end of input means the owner is gone.
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let Ok(show) = serde_json::from_str::<Show>(&line) else {
                continue;
            };
            let route = Route::from(show.route);
            handle.update(move |s, tx| {
                s.navigate(route);
                tx.set_title(show.title);
                say(&format!("shown {}", route.url(&s.base_url)));
            });
            if !hidden {
                handle.open();
            }
        }
        handle.quit();
    });
    app.run();
    background.abort();
    #[cfg(debug_assertions)]
    crate::window_control::unlink();
    ExitCode::SUCCESS
}

fn say(message: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn test_mode() -> String {
    #[cfg(debug_assertions)]
    if let Ok(mode) = std::env::var("KOSSE_TEST_WINDOW")
        && matches!(mode.as_str(), "headless" | "headless-once" | "transparent")
    {
        return mode;
    }
    String::new()
}

/// Debug builds: test reports for a hidden window (`loaded`, `dialog`) and, only when the
/// control socket is enabled, the rendered element tree it serves. Neither runs otherwise.
#[cfg(debug_assertions)]
fn instrument(app: App<State>, hidden: bool, once: bool) -> App<State> {
    let control = crate::window_control::enabled();
    if !hidden && !control {
        return app;
    }
    let tree = std::sync::Arc::new(std::sync::Mutex::new(serde_json::Value::Null));
    let app = {
        let tree = tree.clone();
        let quit = app.handle();
        let mut reported = 0;
        app.on_frame_drawn(move |rt| {
            if hidden && reported != rt.state.route_generation && settled(rt) {
                reported = rt.state.route_generation;
                report_loaded(rt);
                if once {
                    quit.quit();
                }
            }
            if control {
                *tree
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = element_tree(rt);
            }
        })
    };
    if control {
        let control = app.handle();
        crate::window_control::serve(move |request, reply| {
            crate::window_control::dispatch(request, reply, &control, &tree);
            true
        });
    }
    app
}

/// The latest route is applied and drawn: connected, resolved, its dialog (if any) on screen.
#[cfg(debug_assertions)]
fn settled(rt: &rune::Runtime<State>) -> bool {
    let s = &rt.state;
    s.route_generation > 0
        && s.connected
        && s.route.is_none()
        && s.confirmation.is_some() == rt.elements().iter().any(|e| e.role == "dialog")
}

#[cfg(debug_assertions)]
fn report_loaded(rt: &rune::Runtime<State>) {
    let s = &rt.state;
    say(&format!("loaded {}", s.current.url(&s.base_url)));
    let dialog = rt
        .elements()
        .iter()
        .find(|e| e.role == "dialog")
        .and_then(|e| e.label.clone())
        .or_else(|| {
            let id = s.selected?;
            let observation = s.snapshot.as_ref()?.observation.as_ref()?;
            let service = observation
                .services
                .iter()
                .find(|service| service.process.identity == id)?;
            service.process.name.clone()
        });
    if let Ok(value) = serde_json::to_string(&dialog) {
        say(&format!("dialog {value}"));
    }
}

#[cfg(debug_assertions)]
fn element_tree(rt: &rune::Runtime<State>) -> serde_json::Value {
    let elements: Vec<_> = rt
        .elements()
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "key": e.key,
                "path": e.path,
                "role": e.role,
                "text": e.text,
                "label": e.label,
                "rect": e.rect,
                "opacity": e.opacity,
                "focused": e.focused,
                "inert": e.inert,
            })
        })
        .collect();
    serde_json::json!({
        "search": rt.state.search.text,
        "revision": rt.state.snapshot.as_ref().map(|snapshot| snapshot.revision),
        "elements": elements,
    })
}
