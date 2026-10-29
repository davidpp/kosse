//! The menu-bar app, `kosse open` and second launches through the real binary.
//!
//! Debug-build test hooks keep the developer's desktop untouched: `KOSSE_TEST_WINDOW=headless`
//! runs the real native dashboard window (Rune, on the real server) invisibly and reports each
//! route it finished showing; `KOSSE_TEST_OPENER` replaces `/usr/bin/open` with a script that records
//! the URL; `KOSSE_TEST_TRAY=1` prints each rendered tray menu and takes `activate <id>` on stdin,
//! dispatched exactly like a click. The tray icon itself is real and appears briefly in the menu
//! bar. Its look and real clicks are the human checklist in docs/evidence/S08.md.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use kosse_core::model::Snapshot;
use kosse_fixtures::{Fixture, FixtureSpec, live_start_time, protocol::Serve, repo::Sandbox};
use support::{NO_CONFIG, OwnedChild, RawRequest, Server, eventually};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// First scan, the fixture's probe, the next scan, the menu.
const MENU_WITHIN: Duration = Duration::from_secs(15);
const LOAD_WITHIN: Duration = Duration::from_secs(15);

/// A recording stand-in for `/usr/bin/open`: appends each URL to `opened`.
struct Opener {
    _sandbox: Sandbox,
    script: PathBuf,
    opened: PathBuf,
}

impl Opener {
    fn new() -> Self {
        let sandbox = Sandbox::new().expect("sandbox");
        let opened = sandbox.root().join("opened.txt");
        let script = sandbox
            .file(
                "open.sh",
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n",
                    opened.display()
                ),
            )
            .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        Self {
            _sandbox: sandbox,
            script,
            opened,
        }
    }

    fn urls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.opened)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// A spawned `kosse` whose stdout and stderr lines arrive on channels. Its default browser is
/// always an [`Opener`], so no test can open a real browser.
struct Kosse {
    child: OwnedChild,
    opener: Opener,
    stdin: Option<ChildStdin>,
    stdout: mpsc::Receiver<String>,
    stderr: mpsc::Receiver<String>,
    seen_stderr: Vec<String>,
}

impl Kosse {
    fn spawn(args: &[&str], env: &[(&str, &str)]) -> Self {
        let opener = Opener::new();
        let mut command = Command::new(env!("CARGO_BIN_EXE_kosse"));
        command
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .env("KOSSE_TEST_WINDOW", "headless")
            .env("KOSSE_TEST_OPENER", &opener.script)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn kosse");
        let stdin = child.stdin.take();
        let stdout = lines(child.stdout.take().expect("stdout"));
        let stderr = lines(child.stderr.take().expect("stderr"));
        Self {
            child: OwnedChild(child),
            opener,
            stdin,
            stdout,
            stderr,
            seen_stderr: Vec::new(),
        }
    }

    fn pid(&self) -> u32 {
        self.child.0.id()
    }

    /// The `http://127.0.0.1:<port>/` from the startup line.
    fn listening(&mut self) -> String {
        let line = self.stderr_line(|line| line.contains("listening on"));
        line.split("listening on ")
            .nth(1)
            .expect("url")
            .trim()
            .to_owned()
    }

    fn stderr_line(&mut self, wanted: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + LOAD_WITHIN;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.stderr.recv_timeout(left) {
                Ok(line) => {
                    self.seen_stderr.push(line.clone());
                    if wanted(&line) {
                        return line;
                    }
                }
                Err(error) => panic!("no such stderr line ({error}); saw {:#?}", self.seen_stderr),
            }
        }
    }

    fn stdout_line(&self, wanted: impl Fn(&str) -> bool, within: Duration) -> String {
        let deadline = Instant::now() + within;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.stdout.recv_timeout(left) {
                Ok(line) if wanted(&line) => return line,
                Ok(line) => seen.push(line),
                Err(error) => panic!("no such stdout line ({error}); saw {seen:#?}"),
            }
        }
    }

    /// The next complete tray menu that satisfies `wanted`.
    fn menu(&self, wanted: impl Fn(&[String]) -> bool) -> Vec<String> {
        let deadline = Instant::now() + MENU_WITHIN;
        let mut menu = Vec::new();
        let mut last = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(line) = self.stdout.recv_timeout(left) else {
                panic!("no matching menu; the last one was {last:#?}");
            };
            if line.starts_with("tray: icon") {
                menu.clear();
            }
            if line == "tray: end" {
                if wanted(&menu) {
                    return menu;
                }
                last = std::mem::take(&mut menu);
            } else if line.starts_with("tray: ") {
                menu.push(line);
            }
        }
    }

    fn activate(&mut self, id: &str) {
        let stdin = self.stdin.as_mut().expect("stdin");
        writeln!(stdin, "activate {id}").expect("write activate");
        stdin.flush().expect("flush");
    }

    fn exit_code(&mut self) -> Option<i32> {
        self.child.wait_with_timeout().code()
    }

    /// Everything on stderr until the process exits.
    fn rest_of_stderr(&mut self) -> Vec<String> {
        let mut all = std::mem::take(&mut self.seen_stderr);
        all.extend(self.stderr.iter());
        all
    }
}

fn lines(stream: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    receiver
}

/// The dashboard window processes whose parent is `parent`, from `ps` (observation only; the
/// scanner's own short-lived `git` children are not windows).
fn children_of(parent: u32) -> Vec<u32> {
    let output = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid: u32 = fields.next()?.parse().ok()?;
            let parent_pid: u32 = fields.next()?.parse().ok()?;
            let is_window = fields.next()?.ends_with("/kosse") && fields.next() == Some("window");
            (parent_pid == parent && is_window).then_some(pid)
        })
        .collect()
}

/// Whether any process runs a dashboard window for `url` (observation only).
fn window_running_for(url: &str) -> bool {
    let output = Command::new("/bin/ps")
        .args(["-A", "-o", "command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.contains(" window ") && line.contains(url))
}

fn port_of(url: &str) -> u16 {
    url.trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .unwrap_or_else(|| panic!("no port in {url}"))
}

/// The PIDs listening on `port`, from `lsof` (independent ground truth).
fn listening_pids(port: u16) -> Vec<u32> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-t", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .expect("lsof");
    let mut pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// A project directory, so the fixture is attributed and probed (with `--probe-only`).
fn project() -> (Sandbox, PathBuf) {
    let sandbox = Sandbox::new().expect("sandbox");
    let manifest = sandbox.file("shop/package.json", "{}").expect("manifest");
    let dir = manifest.parent().expect("dir").to_path_buf();
    (sandbox, dir)
}

/// The fixture's service, under its project heading, with Open / Inspect / Stop… (by ID), and the
/// fixed items.
fn assert_service_menu(menu: &[String], service: &str, ids: [&str; 3]) {
    assert_eq!(menu[0], "tray: icon default", "{menu:#?}");
    let heading = menu
        .iter()
        .position(|line| line == "tray: text shop")
        .unwrap_or_else(|| panic!("{menu:#?}"));
    for (id, label) in ids.into_iter().zip(["Open", "Inspect", "Stop…"]) {
        let line = format!("tray: item {id}\t{service} > {label}");
        let at = menu
            .iter()
            .position(|item| item == &line)
            .unwrap_or_else(|| panic!("{line:?} not in {menu:#?}"));
        assert!(at > heading, "{menu:#?}");
    }
    for fixed in [
        "tray: item dashboard\tOpen Dashboard",
        "tray: item quit\tQuit Kossé",
    ] {
        assert!(menu.iter().any(|line| line == fixed), "{menu:#?}");
    }
}

/// Activates a menu item and waits until the window loaded `url` and reported `dialog`.
fn activate_and_see(app: &mut Kosse, id: &str, url: &str, dialog: &str) {
    app.activate(id);
    app.stdout_line(|line| line == format!("window: loaded {url}"), LOAD_WITHIN);
    assert_eq!(
        app.stdout_line(|line| line.starts_with("window: dialog "), LOAD_WITHIN),
        format!("window: dialog {dialog}"),
        "{url}"
    );
}

#[test]
fn the_menu_bar_app_routes_every_item_and_quit_leaves_services_running() {
    let (_sandbox, dir) = project();
    let fixture = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .serve(Serve::Title)
            .cwd(&dir),
    )
    .expect("fixture");
    let port = fixture.record().endpoints[0].port();
    let start = fixture.record().started_at_secs;
    let mut app = Kosse::spawn(
        &[
            "--no-open",
            "--port",
            "0",
            &format!("--probe-only={}", fixture.pid()),
            "--act-only=",
        ],
        &[("KOSSE_TEST_TRAY", "1"), ("KOSSE_TEST_WINDOW", "headless")],
    );
    let url = app.listening();
    let origin = url.trim_end_matches('/');

    let open_id = format!("open:http://localhost:{port}/");
    let inspect_id = format!("/?inspect={}.{start}", fixture.pid());
    let stop_id = format!("/?stop={}.{start}", fixture.pid());
    let menu = app.menu(|menu| {
        menu.iter()
            .any(|line| line.starts_with(&format!("tray: item {open_id}\t")))
    });
    assert_service_menu(
        &menu,
        &format!("shop :{port}"),
        [&open_id, &inspect_id, &stop_id],
    );
    assert_eq!(
        children_of(app.pid()),
        Vec::<u32>::new(),
        "no window before one is asked for"
    );

    // Inspect and Stop… open dashboard routes in the one window; Stop… is the dashboard's own
    // confirmation, so nothing is signalled here.
    // The real dashboard (S07) opens that service's Inspect sheet, then its own Stop
    // confirmation (which nothing here confirms), then no dialog.
    activate_and_see(
        &mut app,
        &inspect_id,
        &format!("{origin}{inspect_id}"),
        "\"kosse-fixture\"",
    );
    let windows = children_of(app.pid());
    assert_eq!(windows.len(), 1, "{windows:?}");
    activate_and_see(
        &mut app,
        &stop_id,
        &format!("{origin}{stop_id}"),
        "\"Stop this process?\"",
    );
    activate_and_see(&mut app, "dashboard", &format!("{origin}/"), "null");
    assert_eq!(
        children_of(app.pid()),
        windows,
        "focus-or-spawn reuses the window"
    );
    assert_eq!(
        live_start_time(fixture.pid()),
        Some(start),
        "Stop… signals nothing"
    );

    // Open goes to the default browser.
    app.activate(&open_id);
    let opened = eventually(Duration::from_secs(5), || {
        let urls = app.opener.urls();
        (!urls.is_empty()).then_some(urls)
    });
    assert_eq!(opened, Some(vec![format!("http://localhost:{port}/")]));

    app.activate("quit");
    assert_eq!(app.exit_code(), Some(0));
    let stderr = app.rest_of_stderr();
    assert!(
        stderr.iter().any(|line| line == "kosse stopped"),
        "{stderr:#?}"
    );
    // The window closed with the app; the observed service did not.
    let window = windows[0];
    assert!(
        eventually(Duration::from_secs(3), || (!children_of(app.pid())
            .contains(&window)
            && !window_running_for(origin))
        .then_some(()))
        .is_some(),
        "the window outlived the app"
    );
    assert_eq!(
        live_start_time(fixture.pid()),
        Some(start),
        "quit left the fixture running"
    );
    assert_eq!(listening_pids(port), vec![fixture.pid()]);
    assert_eq!(
        listening_pids(port_of(&url)),
        Vec::<u32>::new(),
        "the app's port is released"
    );
}

#[test]
fn a_second_launch_refocuses_the_menu_bar_app_without_a_second_window_or_scanner() {
    let (app, url) = bare_app("headless");
    let origin = url.trim_end_matches('/');
    let port = port_of(&url).to_string();
    let before = snapshot_at(origin.trim_start_matches("http://"));

    let mut quiet = Kosse::spawn(&["--no-open", "--port", &port], &[]);
    assert_eq!(quiet.exit_code(), Some(0));
    assert_eq!(
        quiet.rest_of_stderr(),
        [format!(
            "kosse: Kossé {} is already running at {url}",
            env!("CARGO_PKG_VERSION")
        )]
    );
    assert_eq!(
        children_of(app.pid()),
        Vec::<u32>::new(),
        "--no-open shows nothing"
    );

    let mut second = Kosse::spawn(&["--port", &port], &[]);
    assert_eq!(second.exit_code(), Some(0));
    assert_eq!(
        second.rest_of_stderr(),
        [format!(
            "kosse: Kossé {} is already running at {url}; showing its window",
            env!("CARGO_PKG_VERSION")
        )]
    );
    app.stdout_line(
        |line| line == format!("window: loaded {origin}/"),
        LOAD_WITHIN,
    );
    let windows = children_of(app.pid());
    assert_eq!(windows.len(), 1, "{windows:?}");

    // `kosse open` asks the same app; the window endpoint accepts one request per second.
    std::thread::sleep(Duration::from_millis(1100));
    let mut open = Kosse::spawn(&["open", "--port", &port], &[]);
    assert_eq!(open.exit_code(), Some(0));
    assert_eq!(
        open.rest_of_stderr(),
        [format!("kosse: showing the native window at {url}")]
    );
    app.stdout_line(
        |line| line == format!("window: loaded {origin}/"),
        LOAD_WITHIN,
    );
    assert_eq!(
        children_of(app.pid()),
        windows,
        "the same window, refocused"
    );
    for launch in [&second, &open, &quiet] {
        assert_eq!(launch.opener.urls(), Vec::<String>::new(), "no browser");
    }
    // Still the one instance and its one scanner, on the one listener.
    let authority = origin.trim_start_matches("http://");
    assert_eq!(snapshot_at(authority).instance, before.instance);
    assert_eq!(listening_pids(port_of(&url)), vec![app.pid()]);

    // `list` reads that instance's snapshot instead of scanning.
    let listed = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", NO_CONFIG)
        .env("KOSSE_TEST_WINDOW", "headless")
        .args(["list", "--json", "--port", &port])
        .output()
        .expect("run list");
    assert!(listed.status.success());
    let listed: Snapshot = serde_json::from_slice(&listed.stdout).expect("snapshot");
    assert_eq!(listed.instance, before.instance);
    assert_eq!(
        listed.scanner.interval_ms,
        Some(3000),
        "the resident scanner's snapshot"
    );
}

#[test]
fn the_window_endpoint_is_guarded_and_routes_to_the_one_window() {
    let (app, url) = bare_app("headless");
    let origin = url.trim_end_matches('/');
    let authority = origin.trim_start_matches("http://");
    let addr = authority.parse().expect("address");
    let post = |token: Option<&str>, body: &str, origin_header: &str| {
        let mut request = RawRequest::new("POST", "/api/window")
            .host(authority)
            .header("Origin", origin_header)
            .header("Content-Type", "application/json");
        if let Some(token) = token {
            request = request.header("X-Kosse-Token", token);
        }
        request.body(body).send(addr)
    };
    let session = RawRequest::new("GET", "/api/session")
        .host(authority)
        .send(addr);
    let token: kosse_core::contract::Session =
        serde_json::from_slice(&session.body).expect("session");
    let inspect = r#"{"route":{"kind":"inspect","pid":4242,"startedAtSecs":1761000000}}"#;

    assert_eq!(post(None, inspect, origin).status, 403, "no token");
    assert_eq!(
        post(Some(&token.token), inspect, "http://evil.example").status,
        403
    );
    assert_eq!(
        post(Some(&token.token), r#"{"route":"/etc"}"#, origin).status,
        400
    );
    assert_eq!(
        children_of(app.pid()),
        Vec::<u32>::new(),
        "refusals show nothing"
    );

    let accepted = post(Some(&token.token), inspect, origin);
    assert_eq!(accepted.status, 204, "{}", accepted.text());
    // Straight after the accepted request, before waiting for the window: loading it can take
    // longer than the one-second limit on a slow machine.
    assert_eq!(
        post(Some(&token.token), inspect, origin).status,
        429,
        "one per second"
    );
    app.stdout_line(
        |line| line == format!("window: loaded {origin}/?inspect=4242.1761000000"),
        LOAD_WITHIN,
    );

    // A foreground owner also accepts native window requests.
    let server = Server::start();
    let accepted = server.post_json("/api/window", &server.token(), inspect);
    assert_eq!(accepted.status, 204, "{}", accepted.text());
}

#[test]
fn a_second_launch_never_duplicates_a_headless_or_ephemeral_instance() {
    // `serve`: no window to show, so the browser opens it, with the reason.
    let server = Server::start();
    let port = server.addr.port().to_string();
    let url = format!("http://{}/", server.addr);
    let mut quiet = Kosse::spawn(&["--no-open", "--port", &port], &[]);
    assert_eq!(quiet.exit_code(), Some(0));
    assert_eq!(
        quiet.rest_of_stderr(),
        [format!(
            "kosse: `kosse serve` is already running at {url} (no menu-bar app)"
        )]
    );
    for args in [vec!["--port", &port], vec!["open", "--port", &port]] {
        let mut launch = Kosse::spawn(&args, &[]);
        assert_eq!(launch.exit_code(), Some(0), "{:?}", launch.rest_of_stderr());
        assert_eq!(launch.opener.urls(), Vec::<String>::new());
        thread::sleep(Duration::from_millis(1100));
    }
    assert_eq!(
        children_of(server.child.0.id()).len(),
        1,
        "one reused native window"
    );
    assert_eq!(
        listening_pids(server.addr.port()),
        vec![server.child.0.id()]
    );

    // An ephemeral `kosse open`: the app refuses clearly; another `open` gets a browser tab.
    let mut ephemeral = Kosse::spawn(
        &["open", "--port", "0", "--probe-only=", "--act-only="],
        &[("KOSSE_TEST_WINDOW", "headless")],
    );
    let url = ephemeral.listening();
    let port = port_of(&url).to_string();
    let mut app = Kosse::spawn(&["--port", &port], &[]);
    assert_eq!(app.exit_code(), Some(1));
    assert_eq!(
        app.rest_of_stderr(),
        [format!(
            "kosse: the Kossé at {url} is a `kosse open` session, which stops when its window closes; close that window, then start kosse again"
        )]
    );
    let mut open = Kosse::spawn(&["open", "--port", &port], &[]);
    assert_eq!(open.exit_code(), Some(0));
    assert_eq!(open.opener.urls(), Vec::<String>::new());
    let status = Command::new("kill")
        .args(["-TERM", &ephemeral.pid().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    assert_eq!(ephemeral.exit_code(), Some(0));
}

#[test]
fn a_killed_owner_takes_its_window_with_it() {
    let mut open = Kosse::spawn(
        &["open", "--port", "0", "--probe-only=", "--act-only="],
        &[("KOSSE_TEST_WINDOW", "headless")],
    );
    let url = open.listening();
    open.stdout_line(
        |line| line == format!("window: loaded {}/", url.trim_end_matches('/')),
        LOAD_WITHIN,
    );
    let windows = children_of(open.pid());
    assert_eq!(windows.len(), 1, "{windows:?}");
    // SIGKILL to the process this test started: no cleanup code runs in it.
    let status = Command::new("kill")
        .args(["-KILL", &open.pid().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    let _ = open.exit_code();
    assert!(
        eventually(Duration::from_secs(5), || (!window_running_for(&url))
            .then_some(()))
        .is_some(),
        "the window outlived its killed owner"
    );
}

#[test]
fn native_window_failure_is_reported_in_the_tray() {
    let (mut app, url) = bare_app("fail");
    app.activate("dashboard");
    assert_eq!(app.opener.urls(), Vec::<String>::new());
    let notice = "tray: text Dashboard window unavailable: the dashboard window did not start (disabled by KOSSE_TEST_WINDOW=fail)";
    let menu = app.menu(|menu| menu.iter().any(|line| line == notice));
    assert_eq!(menu[1], notice, "{menu:#?}");
    let stderr = app.stderr_line(|line| line.contains("did not start"));
    assert_eq!(
        stderr,
        format!(
            "kosse: the dashboard window did not start (disabled by KOSSE_TEST_WINDOW=fail); native window unavailable for {url}. Retry with `kosse open`."
        )
    );
    // The notice is one-shot: the next click clears it.
    app.activate("quit");
    assert_eq!(app.exit_code(), Some(0));
    let duplicate = app
        .rest_of_stderr()
        .into_iter()
        .filter(|line| line.contains("KOSSE_TEST_WINDOW=fail"))
        .count();
    assert_eq!(
        duplicate, 1,
        "the window child writes no stderr line of its own"
    );
}

#[test]
fn an_unrelated_port_occupant_is_never_trusted() {
    for serve in [Serve::Title, Serve::Banner] {
        let occupant =
            Fixture::spawn(&FixtureSpec::new().listen(&[V4]).serve(serve)).expect("fixture");
        let port = occupant.record().endpoints[0].port().to_string();
        for args in [
            vec!["--no-open", "--port", &port],
            vec!["open", "--port", &port],
        ] {
            let mut kosse = Kosse::spawn(&args, &[("KOSSE_TEST_WINDOW", "fail")]);
            assert_eq!(kosse.exit_code(), Some(1), "{args:?}");
            let stderr = kosse.rest_of_stderr();
            assert_eq!(
                stderr,
                [format!(
                    "kosse: 127.0.0.1:{port} is in use by a process that is not Kossé; stop it or choose another port with --port"
                )],
                "{args:?}"
            );
        }
        let listed = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .env("KOSSE_TEST_WINDOW", "headless")
            .args(["list", "--json", "--port", &port])
            .output()
            .expect("run list");
        assert!(listed.status.success());
        let listed: Snapshot = serde_json::from_slice(&listed.stdout).expect("snapshot");
        assert_eq!(
            (listed.revision, listed.scanner.interval_ms),
            (1, None),
            "a one-shot scan"
        );
        assert_eq!(
            live_start_time(occupant.pid()),
            Some(occupant.record().started_at_secs)
        );
    }
}

#[test]
fn open_serves_until_its_window_closes_then_leaves_nothing_running() {
    let mut open = Kosse::spawn(
        &["open", "--port", "0", "--probe-only=", "--act-only="],
        &[("KOSSE_TEST_WINDOW", "headless-once")],
    );
    let url = open.listening();
    let origin = url.trim_end_matches('/');
    open.stdout_line(
        |line| line == format!("window: loaded {origin}/"),
        LOAD_WITHIN,
    );
    assert_eq!(open.exit_code(), Some(0));
    let stderr = open.rest_of_stderr();
    assert_eq!(
        stderr.last().map(String::as_str),
        Some("kosse stopped"),
        "{stderr:#?}"
    );
    assert_eq!(listening_pids(port_of(&url)), Vec::<u32>::new());
    assert!(!window_running_for(&url));
}

#[test]
fn a_window_that_cannot_start_reports_failure_and_stops_its_owner() {
    let mut open = Kosse::spawn(
        &["open", "--port", "0", "--probe-only=", "--act-only="],
        &[("KOSSE_TEST_WINDOW", "fail")],
    );
    let url = open.listening();
    let reason = open.stderr_line(|line| line.contains("did not start"));
    assert_eq!(
        reason,
        format!(
            "kosse: the dashboard window did not start (disabled by KOSSE_TEST_WINDOW=fail); native window unavailable for {url}. Retry with `kosse open`."
        )
    );
    assert_eq!(open.opener.urls(), Vec::<String>::new());
    assert_eq!(open.exit_code(), Some(1));
    assert_eq!(listening_pids(port_of(&url)), Vec::<u32>::new());
}

#[test]
fn headless_serve_has_no_tray() {
    let mut serve = Kosse::spawn(
        &[
            "serve",
            "--no-open",
            "--port",
            "0",
            "--probe-only=",
            "--act-only=",
        ],
        &[("KOSSE_TEST_TRAY", "1")],
    );
    let url = serve.listening();
    let authority = url.trim_start_matches("http://").trim_end_matches('/');
    let health = RawRequest::new("GET", "/api/health")
        .host(authority)
        .send(authority.parse().expect("address"));
    assert_eq!(health.status, 200);
    let status = Command::new("kill")
        .args(["-TERM", &serve.pid().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    assert_eq!(serve.exit_code(), Some(0));
    let stdout: Vec<String> = serve.stdout.iter().collect();
    assert_eq!(stdout, Vec::<String>::new(), "no tray menu was rendered");
}

/// The menu-bar app with no fixture of its own: it probes and may signal nothing. `window` is its
/// `KOSSE_TEST_WINDOW` mode.
fn bare_app(window: &str) -> (Kosse, String) {
    let mut app = Kosse::spawn(
        &["--no-open", "--port", "0", "--probe-only=", "--act-only="],
        &[("KOSSE_TEST_TRAY", "1"), ("KOSSE_TEST_WINDOW", window)],
    );
    let url = app.listening();
    app.menu(|menu| {
        menu.iter()
            .any(|line| line == "tray: item quit\tQuit Kossé")
    });
    (app, url)
}

#[test]
fn the_menu_bar_app_stops_cleanly_on_sigterm() {
    let (mut app, url) = bare_app("headless");
    let status = Command::new("kill")
        .args(["-TERM", &app.pid().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    assert_eq!(app.exit_code(), Some(0));
    let stderr = app.rest_of_stderr();
    assert!(
        stderr.iter().any(|line| line == "kosse stopped"),
        "{stderr:#?}"
    );
    assert_eq!(listening_pids(port_of(&url)), Vec::<u32>::new());
}

#[test]
fn a_crashed_window_leaves_the_scanner_running_and_is_replaced() {
    let (mut app, url) = bare_app("headless");
    let origin = url.trim_end_matches('/');
    app.activate("dashboard");
    app.stdout_line(
        |line| line == format!("window: loaded {origin}/"),
        LOAD_WITHIN,
    );
    let windows = children_of(app.pid());
    assert_eq!(windows.len(), 1, "{windows:?}");
    let window = windows[0];
    // The app's own child, started at this test's request: confirmed by parent and command line
    // before it is "crashed" with SIGKILL.
    let command = Command::new("/bin/ps")
        .args(["-o", "command=", "-p", &window.to_string()])
        .output()
        .expect("ps");
    let command = String::from_utf8_lossy(&command.stdout);
    assert!(command.contains(&format!(" window {url}")), "{command}");
    let status = Command::new("kill")
        .args(["-KILL", &window.to_string()])
        .status()
        .expect("kill");
    assert!(status.success());

    let authority = origin.trim_start_matches("http://");
    let before = snapshot_at(authority);
    let advanced = eventually(Duration::from_secs(10), || {
        (snapshot_at(authority).revision > before.revision).then_some(())
    });
    assert!(advanced.is_some(), "the scanner kept publishing");

    app.activate("dashboard");
    app.stdout_line(
        |line| line == format!("window: loaded {origin}/"),
        LOAD_WITHIN,
    );
    let replaced = children_of(app.pid());
    assert_eq!(replaced.len(), 1, "{replaced:?}");
    assert_ne!(replaced[0], window, "a new window replaced the crashed one");
    app.activate("quit");
    assert_eq!(app.exit_code(), Some(0));
}

fn snapshot_at(authority: &str) -> Snapshot {
    let response = RawRequest::new("GET", "/api/snapshot")
        .host(authority)
        .send(authority.parse().expect("address"));
    assert_eq!(response.status, 200);
    serde_json::from_slice(&response.body).expect("snapshot")
}
