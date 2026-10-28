//! `kosse`: the menu-bar app. The tray icon and its menu live on the main thread, in tao's event
//! loop (`AppKit` requires it); the instance (scanner thread, tokio runtime, HTTP server) runs on
//! background threads, exactly as `serve` runs it.
//!
//! - A tokio task turns each published snapshot into a [`menu::Model`] and sends it to the main
//!   thread only when it changed; the main thread renders it. The tray never scans.
//! - Menu clicks arrive as events: Open goes to the default browser, Inspect / Stop… / Open
//!   Dashboard to the one dashboard window (focus-or-spawn), Quit stops the instance.
//! - `POST /api/window` (a second `kosse`, or `kosse open`) arrives the same way and shows the
//!   same window: there is only ever one.
//! - Quit, SIGINT and SIGTERM all end the same way: the server drains, the scanner stops, the
//!   window closes, then the loop exits. Observed services are never touched.

use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use kosse_core::contract::InstanceMode;
use muda::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tao::{
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::macos::{ActivationPolicy, EventLoopExtMacOS},
};
use tokio::sync::oneshot;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::{
    AppError, activation,
    api::WindowSink,
    cli::AppArgs,
    instance::{self, Instance, InstanceError},
    launcher::{self, Notice, Windows},
    menu::{Action, Item, Model},
    opener,
    route::Route,
    running::{self, Claim},
    scheduler::SchedulerHandle,
};

/// How often the menu is re-evaluated without a publication, so a stalled scanner shows.
const STALL_CHECK: Duration = Duration::from_secs(5);

const DEFAULT_ICON: &[u8] = include_bytes!("../../../brand/tray/tray-default@2x.png");
const ATTENTION_ICON: &[u8] = include_bytes!("../../../brand/tray/tray-attention@2x.png");

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("could not create the menu-bar icon: {0}")]
    Icon(String),
}

enum Message {
    Model(Model),
    Menu(MenuId),
    /// `POST /api/window`.
    Window(Route),
    WindowNotice(Notice),
    /// The instance stopped; `true` when cleanly.
    Stopped(bool),
}

pub fn run(args: &AppArgs) -> Result<(), AppError> {
    let runtime = crate::runtime()?;
    // Built before the instance: the window endpoint sends to this loop.
    let mut event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    // A menu-bar app: no Dock icon, no app menu of its own.
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy = event_loop.create_proxy();
    let window_proxy = Mutex::new(proxy.clone());
    let window: WindowSink = Arc::new(move |route| {
        let proxy = window_proxy.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = proxy.send_event(Message::Window(route.into()));
    });
    let running =
        match running::claim(&runtime, &args.server, InstanceMode::Resident, Some(window))? {
            Claim::Existing(existing) => return second_launch(&runtime, &existing, args.no_open),
            Claim::Started(running) => running,
        };
    let url = running.url().to_owned();
    report!("kosse {} listening on {url}", env!("CARGO_PKG_VERSION"));
    let icons = Icons::load()?;

    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_proxy.send_event(Message::Menu(event.id));
    }));
    #[cfg(debug_assertions)]
    test_control::start(&proxy);

    runtime.spawn(watch(running.snapshots(), proxy.clone()));
    let (quit, quit_requested) = oneshot::channel::<()>();
    let stopped = proxy.clone();
    runtime.spawn(async move {
        let result = running
            .run(async move {
                let _ = quit_requested.await;
            })
            .await;
        if let Err(error) = &result {
            report!("kosse: {error}");
        }
        let _ = stopped.send_event(Message::Stopped(result.is_ok()));
    });

    let notices = Mutex::new(proxy);
    let mut app = App {
        url,
        icons,
        tray: None,
        actions: HashMap::new(),
        model: Model::of(None, 0),
        notice: None,
        windows: Windows::new(Arc::new(move |notice| {
            let proxy = notices.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = proxy.send_event(Message::WindowNotice(notice));
        })),
        quit: Some(quit),
        failed: false,
    };
    let open_at_launch = !args.no_open;
    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        // The runtime runs the instance; it lives as long as the loop.
        let _runtime = &runtime;
        match event {
            Event::NewEvents(StartCause::Init) => {
                // tray-icon requires the loop to be running before the status item is created.
                if let Err(error) = app.create_tray() {
                    report!("kosse: {error}");
                    app.failed = true;
                    app.quit();
                } else if open_at_launch {
                    app.show(Route::Dashboard);
                }
            }
            Event::UserEvent(Message::Model(model)) => app.update(model),
            Event::UserEvent(Message::Menu(id)) => app.activate(&id),
            Event::UserEvent(Message::Window(route)) => app.show(route),
            Event::UserEvent(Message::WindowNotice(Notice::Unavailable { reason })) => {
                app.notice = Some(format!(
                    "Dashboard window unavailable: {reason} (opened in the browser)"
                ));
                app.render();
            }
            Event::UserEvent(Message::WindowNotice(Notice::Lost { url, title })) => {
                app.windows.show(&url, &title);
            }
            Event::UserEvent(Message::Stopped(clean)) => {
                app.windows.close();
                app.tray = None;
                report!("kosse stopped");
                *control_flow = ControlFlow::ExitWithCode(i32::from(!clean || app.failed));
            }
            _ => {}
        }
    })
}

/// A Kossé of this user already serves the port: show the menu-bar app's own window, or say
/// why there is none. Never a second scanner or window.
fn second_launch(
    runtime: &tokio::runtime::Runtime,
    existing: &Instance,
    no_open: bool,
) -> Result<(), AppError> {
    let url = &existing.url;
    match existing.mode {
        InstanceMode::Resident if no_open => {
            report!(
                "kosse: Kossé {} is already running at {url}",
                existing.version
            );
        }
        InstanceMode::Resident => {
            runtime.block_on(instance::show_window(existing, Route::Dashboard))?;
            report!(
                "kosse: Kossé {} is already running at {url}; showing its window",
                existing.version
            );
        }
        InstanceMode::Ephemeral => return Err(InstanceError::Ephemeral(url.clone()).into()),
        InstanceMode::Headless if no_open => {
            report!("kosse: `kosse serve` is already running at {url} (no menu-bar app)");
        }
        InstanceMode::Headless => launcher::fallback(
            url,
            &format!("`kosse serve` is already running at {url}, without a menu-bar app or window"),
        ),
    }
    Ok(())
}

/// Recomputes the menu on every publication (and every [`STALL_CHECK`]) and sends it to the main
/// thread when it differs from the last one sent.
async fn watch(snapshots: SchedulerHandle, proxy: EventLoopProxy<Message>) {
    let mut changes = snapshots.subscribe();
    let mut last: Option<Model> = None;
    let mut ticks = tokio::time::interval(STALL_CHECK);
    loop {
        let latest = changes.borrow_and_update().clone();
        let model = Model::of(
            latest.as_deref(),
            kosse_core::scan::unix_millis(std::time::SystemTime::now()),
        );
        if last.as_ref() != Some(&model) {
            if proxy.send_event(Message::Model(model.clone())).is_err() {
                return;
            }
            last = Some(model);
        }
        tokio::select! {
            published = changes.changed() => if published.is_err() { return },
            _ = ticks.tick() => {}
        }
    }
}

struct App {
    url: String,
    icons: Icons,
    tray: Option<TrayIcon>,
    /// What each item of the current menu does, by its ID.
    actions: HashMap<MenuId, Action>,
    model: Model,
    /// Shown above the menu until the next click: why the window fell back to the browser.
    notice: Option<String>,
    windows: Windows,
    quit: Option<oneshot::Sender<()>>,
    /// The tray could not be created: the app stops and exits with 1.
    failed: bool,
}

impl App {
    fn create_tray(&mut self) -> Result<(), TrayError> {
        let (menu, actions) =
            render(&self.items()).map_err(|error| TrayError::Icon(error.to_string()))?;
        let tray = TrayIconBuilder::new()
            .with_icon_templated(self.icons.for_model(&self.model))
            .with_tooltip("Kossé")
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| TrayError::Icon(error.to_string()))?;
        self.tray = Some(tray);
        self.actions = actions;
        Ok(())
    }

    /// The model's items, after the one-shot notice if there is one.
    fn items(&self) -> Vec<Item> {
        let mut items = Vec::with_capacity(self.model.items.len() + 2);
        if let Some(notice) = &self.notice {
            items.push(Item::Text(notice.clone()));
            items.push(Item::Separator);
        }
        items.extend(self.model.items.iter().cloned());
        items
    }

    fn update(&mut self, model: Model) {
        let attention_changed = model.attention.is_some() != self.model.attention.is_some();
        self.model = model;
        let Some(tray) = &self.tray else {
            return;
        };
        if attention_changed
            && let Err(error) = tray.set_icon_templated(Some(self.icons.for_model(&self.model)))
        {
            report!("kosse: could not change the tray icon: {error}");
        }
        let _ = tray.set_tooltip(Some(
            self.model
                .attention
                .as_deref()
                .map_or_else(|| "Kossé".to_owned(), |reason| format!("Kossé: {reason}")),
        ));
        self.render();
    }

    fn render(&mut self) {
        let Some(tray) = &self.tray else {
            return;
        };
        let items = self.items();
        match render(&items) {
            Ok((menu, actions)) => {
                tray.set_menu(Some(Box::new(menu)));
                self.actions = actions;
            }
            Err(error) => report!("kosse: could not rebuild the tray menu: {error}"),
        }
        #[cfg(debug_assertions)]
        test_control::report_menu(self.model.attention.is_some(), &items);
    }

    fn activate(&mut self, id: &MenuId) {
        let Some(action) = self.actions.get(id).cloned() else {
            return;
        };
        if self.notice.take().is_some() {
            self.render();
        }
        match action {
            Action::Open(url) => opener::open_or_report(&url),
            Action::Show { route, .. } => self.show(route),
            Action::Quit => self.quit(),
        }
    }

    /// Shows `route` in the one window and brings it forward; its title names the service when
    /// the current menu knows it.
    fn show(&mut self, route: Route) {
        let title = self
            .model
            .title_for(route)
            .unwrap_or_else(|| launcher::TITLE.to_owned());
        if let Some(pid) = self.windows.show(&route.url(&self.url), &title) {
            activation::bring_forward(pid);
        }
    }

    /// Asks the instance to stop; the loop exits once it has.
    fn quit(&mut self) {
        if let Some(quit) = self.quit.take() {
            let _ = quit.send(());
        }
    }
}

/// The native menu for `items`, and the action behind each clickable item's ID.
fn render(items: &[Item]) -> muda::Result<(Menu, HashMap<MenuId, Action>)> {
    let menu = Menu::new();
    let mut actions = HashMap::new();
    for item in items {
        menu.append(native(item, &mut actions)?.as_ref())?;
    }
    Ok((menu, actions))
}

fn native(item: &Item, actions: &mut HashMap<MenuId, Action>) -> muda::Result<Box<dyn IsMenuItem>> {
    Ok(match item {
        Item::Text(text) => Box::new(MenuItem::new(escape(text), false, None)),
        Item::Heading(text) => Box::new(PredefinedMenuItem::section_header(&escape(text))),
        Item::Separator => Box::new(PredefinedMenuItem::separator()),
        Item::Action { label, action } => {
            let id = MenuId::new(action.id());
            actions.insert(id.clone(), action.clone());
            Box::new(MenuItem::with_id(id, escape(label), true, None))
        }
        Item::Submenu { label, items } => {
            let submenu = Submenu::new(escape(label), true);
            for item in items {
                submenu.append(native(item, actions)?.as_ref())?;
            }
            Box::new(submenu)
        }
    })
}

/// muda treats `&` as a mnemonic marker; `&&` is a literal ampersand.
fn escape(text: &str) -> String {
    text.replace('&', "&&")
}

/// The two template images (black + alpha; macOS tints them for the menu bar).
struct Icons {
    default: Icon,
    attention: Icon,
}

impl Icons {
    fn load() -> Result<Self, TrayError> {
        Ok(Self {
            default: template(DEFAULT_ICON)?,
            attention: template(ATTENTION_ICON)?,
        })
    }

    fn for_model(&self, model: &Model) -> Icon {
        if model.attention.is_some() {
            self.attention.clone()
        } else {
            self.default.clone()
        }
    }
}

/// The 36 px (18 pt @2x) glyph, centred on a transparent 44 px canvas: tray-icon sizes any image
/// taller than 22 pt down to 22 pt, so this keeps the glyph at 18 pt and Retina-sharp.
fn template(png_bytes: &[u8]) -> Result<Icon, TrayError> {
    const CANVAS: u32 = 44;
    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|error| TrayError::Icon(error.to_string()))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| TrayError::Icon("icon too large".to_owned()))?;
    let mut glyph = vec![0; size];
    let frame = reader
        .next_frame(&mut glyph)
        .map_err(|error| TrayError::Icon(error.to_string()))?;
    if frame.color_type != png::ColorType::Rgba
        || frame.bit_depth != png::BitDepth::Eight
        || frame.width > CANVAS
        || frame.height > CANVAS
    {
        return Err(TrayError::Icon("unexpected tray icon format".to_owned()));
    }
    let (width, height) = (frame.width as usize, frame.height as usize);
    let canvas = CANVAS as usize;
    let (left, top) = ((canvas - width) / 2, (canvas - height) / 2);
    let mut rgba = vec![0; canvas * canvas * 4];
    for row in 0..height {
        let source = &glyph[row * frame.line_size..row * frame.line_size + width * 4];
        let start = ((top + row) * canvas + left) * 4;
        rgba[start..start + width * 4].copy_from_slice(source);
    }
    Icon::from_rgba(rgba, CANVAS, CANVAS).map_err(|error| TrayError::Icon(error.to_string()))
}

/// Debug builds only, for the binary-level tests: with `KOSSE_TEST_TRAY=1` the app prints each
/// menu it renders to stdout (`tray: icon …`, `tray: item <id>\t<label>`, `tray: end`) and takes
/// `activate <id>` lines on stdin, delivered exactly like a click on that item. The native menu's
/// look and the click itself remain a human check.
#[cfg(debug_assertions)]
mod test_control {
    use std::io::{self, BufRead, Write};

    use muda::MenuId;
    use tao::event_loop::EventLoopProxy;

    use super::Message;

    fn enabled() -> bool {
        std::env::var_os("KOSSE_TEST_TRAY").is_some()
    }

    pub fn start(proxy: &EventLoopProxy<Message>) {
        if !enabled() {
            return;
        }
        let proxy = proxy.clone();
        let _ = std::thread::Builder::new()
            .name("kosse-test-tray".into())
            .spawn(move || {
                for line in io::stdin().lock().lines() {
                    let Ok(line) = line else { return };
                    if let Some(id) = line.strip_prefix("activate ")
                        && proxy
                            .send_event(Message::Menu(MenuId::new(id.trim())))
                            .is_err()
                    {
                        return;
                    }
                }
            });
    }

    pub fn report_menu(attention: bool, items: &[super::Item]) {
        if !enabled() {
            return;
        }
        let mut out = io::stdout().lock();
        let icon = if attention { "attention" } else { "default" };
        let _ = writeln!(out, "tray: icon {icon}");
        print_items(&mut out, items, "");
        let _ = writeln!(out, "tray: end");
        let _ = out.flush();
    }

    fn print_items(out: &mut impl Write, items: &[super::Item], path: &str) {
        for item in items {
            match item {
                super::Item::Action { label, action } => {
                    let _ = writeln!(out, "tray: item {}\t{path}{label}", action.id());
                }
                super::Item::Submenu { label, items } => {
                    print_items(out, items, &format!("{path}{label} > "));
                }
                super::Item::Text(text) | super::Item::Heading(text) => {
                    let _ = writeln!(out, "tray: text {path}{text}");
                }
                super::Item::Separator => {}
            }
        }
    }
}
