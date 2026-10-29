//! Rune dashboard. The window reads the owner's API; it never scans or signals.
mod client;
mod customize;
mod view;

use crate::cli::WindowArgs;
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
    route: Option<String>,
    current_url: String,
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
            current_url: String::new(),
            route_generation: 1,
        }
    }
}

impl State {
    fn accept(&mut self, snapshot: Snapshot) {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|old| old.instance != snapshot.instance)
        {
            self.ended.clear();
            self.confirmation = None;
            self.snapshot = None;
        }
        if let Some(next) = &snapshot.observation {
            self.ended.retain(|(service, seen)| {
                next.scanned_at_ms.saturating_sub(*seen) < 60_000
                    && !next
                        .services
                        .iter()
                        .any(|s| s.process.identity == service.process.identity)
            });
            if let Some(previous) = self.snapshot.as_ref().and_then(|s| s.observation.as_ref()) {
                for service in &previous.services {
                    if !next
                        .services
                        .iter()
                        .any(|s| s.process.identity == service.process.identity)
                        && !self
                            .ended
                            .iter()
                            .any(|(s, _)| s.process.identity == service.process.identity)
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

    fn navigate(&mut self, url: String) {
        self.current_url.clone_from(&url);
        self.route_generation += 1;
        self.route = Some(url);
        self.apply_route();
    }

    fn apply_route(&mut self) {
        let Some(url) = self.route.as_ref() else {
            return;
        };
        let Some((_, query)) = url.split_once('?') else {
            self.selected = None;
            self.confirmation = None;
            self.route = None;
            return;
        };
        let Some((kind, target)) = query.split_once('=') else {
            self.route = None;
            return;
        };
        let Some((pid, start)) = target.split_once('.') else {
            self.route = None;
            return;
        };
        let (Ok(pid), Ok(start)) = (pid.parse(), start.parse()) else {
            self.route = None;
            return;
        };
        let identity = ProcessIdentity {
            pid,
            started_at_secs: Some(start),
        };
        let Some(observation) = self.snapshot.as_ref().and_then(|s| s.observation.as_ref()) else {
            return;
        };
        let service = observation
            .services
            .iter()
            .find(|s| s.process.identity == identity)
            .cloned();
        let stop = kind == "stop";
        self.route = None;
        self.confirmation = None;
        self.editor = None;
        if let Some(service) = service {
            self.selected = Some(identity);
            if stop {
                self.confirm(service);
            }
        } else {
            self.error = Some("This process is no longer in the current observation.".into());
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
    let url = args.url.clone();
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
            current_url: url.clone(),
            route: Some(url),
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
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let Some((url, title)) = line.split_once('\t') else {
                continue;
            };
            if !url.starts_with(&format!("http://{addr}/")) {
                continue;
            }
            let url = url.to_owned();
            let title = title.to_owned();
            handle.update(move |s, tx| {
                s.navigate(url.clone());
                tx.set_title(title);
                say(&format!("shown {url}"));
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

#[cfg(debug_assertions)]
fn instrument(app: App<State>, hidden: bool, once: bool) -> App<State> {
    let tree = std::sync::Arc::new(std::sync::Mutex::new(serde_json::Value::Null));
    let app = {
        let tree = tree.clone();
        let quit = app.handle();
        let mut reported = 0;
        app.on_frame_drawn(move |rt| {
            if hidden && rt.state.connected && rt.state.route.is_none() && reported != rt.state.route_generation
                && rt.state.confirmation.is_some() == rt.elements().iter().any(|e| e.role == "dialog") {
                reported = rt.state.route_generation;
                say(&format!("loaded {}", rt.state.current_url));
                let dialog = rt.elements().iter().find(|e| e.role == "dialog").and_then(|e| e.label.clone())
                    .or_else(|| rt.state.selected.and_then(|id| rt.state.snapshot.as_ref()?.observation.as_ref()?.services.iter().find(|s| s.process.identity == id)?.process.name.clone()));
                if let Ok(value) = serde_json::to_string(&dialog) { say(&format!("dialog {value}")); }
                if once { quit.quit(); }
            }
            let elements: Vec<_> = rt.elements().into_iter().map(|e| serde_json::json!({"key": e.key, "path": e.path, "role": e.role, "text": e.text, "label": e.label, "rect": e.rect, "opacity": e.opacity, "focused": e.focused, "inert": e.inert})).collect();
            *tree.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = serde_json::json!({"search": rt.state.search.text, "elements": elements});
        })
    };
    let handle = app.handle();
    {
        let control = handle.clone();
        crate::window_control::serve(move |request, reply| {
            crate::window_control::dispatch(request, reply, &control, &tree);
            true
        });
    }
    app
}
