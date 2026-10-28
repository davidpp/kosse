//! Drives the compiled `stackscan` binary over real TCP. Each test owns the child it spawns and
//! stops it by that handle only.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, ChildStderr, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use stackscan_core::contract::Health;

const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// A spawned `stackscan` that is killed (by this handle only) if the test ends early.
struct OwnedChild(Child);

impl OwnedChild {
    fn spawn(args: &[&str]) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_stackscan"))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn stackscan");
        Self(child)
    }

    fn take_stderr(&mut self) -> BufReader<ChildStderr> {
        BufReader::new(self.0.stderr.take().expect("piped stderr"))
    }

    fn wait_with_timeout(&mut self) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.0.try_wait().expect("poll child") {
                return status;
            }
            assert!(Instant::now() < deadline, "stackscan did not exit in time");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Runs to completion (bounded by [`EXIT_TIMEOUT`]) and returns the exit status and stderr.
    fn run(args: &[&str]) -> (ExitStatus, String) {
        let mut child = Self::spawn(args);
        let mut stderr = child.take_stderr();
        let status = child.wait_with_timeout();
        let mut output = String::new();
        stderr.read_to_string(&mut output).expect("read stderr");
        (status, output)
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct Server {
    child: OwnedChild,
    addr: SocketAddr,
    stderr: BufReader<ChildStderr>,
}

impl Server {
    fn start() -> Self {
        let mut child = OwnedChild::spawn(&["serve", "--no-open", "--port", "0"]);
        let mut stderr = child.take_stderr();
        let mut line = String::new();
        stderr.read_line(&mut line).expect("read startup line");
        let addr = line
            .split("listening on http://")
            .nth(1)
            .and_then(|rest| rest.trim().trim_end_matches('/').parse().ok())
            .unwrap_or_else(|| panic!("unexpected startup line: {line:?}"));
        Self {
            child,
            addr,
            stderr,
        }
    }

    fn get(&self, path: &str) -> HttpResponse {
        self.request("GET", path)
    }

    fn request(&self, method: &str, path: &str) -> HttpResponse {
        let mut stream = TcpStream::connect(self.addr).expect("connect");
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            self.addr
        )
        .expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        HttpResponse::parse(&raw)
    }

    /// Sends SIGTERM to the child this test spawned and waits for it to exit.
    fn terminate(mut self) -> (ExitStatus, String) {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.0.id().to_string()])
            .status()
            .expect("run kill");
        assert!(status.success(), "kill -TERM failed");
        let exit = self.child.wait_with_timeout();
        let mut rest = String::new();
        self.stderr.read_to_string(&mut rest).expect("read stderr");
        (exit, rest)
    }
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn parse(raw: &[u8]) -> Self {
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("header terminator");
        let head = std::str::from_utf8(&raw[..split]).expect("utf-8 headers");
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .expect("status line");
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        Self {
            status,
            headers,
            body: raw[split + 4..].to_vec(),
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[test]
fn health_reports_name_and_version() {
    let server = Server::start();
    let response = server.get("/api/health");

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let health: Health = serde_json::from_slice(&response.body).expect("health json");
    assert_eq!(
        health,
        Health {
            name: "stackscan".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    );
}

#[test]
fn serves_embedded_bundle_with_client_route_fallback() {
    let server = Server::start();

    let index = server.get("/");
    assert_eq!(index.status, 200, "{}", index.text());
    assert_eq!(index.header("content-type"), Some("text/html"));
    assert_eq!(index.header("cache-control"), Some("no-cache"));
    let html = index.text();
    assert!(html.contains(r#"<div id="app">"#), "{html}");

    let client_route = server.get("/services/example");
    assert_eq!(client_route.status, 200);
    assert_eq!(client_route.text(), html);

    let script = html
        .split('"')
        .find(|part| {
            part.starts_with("/assets/") && Path::new(part).extension() == Some("js".as_ref())
        })
        .expect("index references a hashed script");
    let asset = server.get(script);
    assert_eq!(asset.status, 200);
    assert_eq!(asset.header("content-type"), Some("text/javascript"));
    assert_eq!(
        asset.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
}

#[test]
fn unknown_files_and_api_routes_are_not_found() {
    let server = Server::start();

    assert_eq!(server.get("/assets/missing.js").status, 404);
    assert_eq!(server.get("/api/unknown").status, 404);
    assert_eq!(server.get("/api").status, 404);
    assert_eq!(server.request("POST", "/").status, 405);
}

fn assert_refuses_port_held_at(occupant_addr: &str) {
    let occupant = TcpListener::bind(occupant_addr).expect("bind occupant");
    let held = occupant.local_addr().expect("occupant addr");

    let (status, stderr) =
        OwnedChild::run(&["serve", "--no-open", "--port", &held.port().to_string()]);

    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains(&format!("{held} is already in use")),
        "{stderr}"
    );
}

#[test]
fn port_held_on_ipv4_loopback_fails_with_clear_error() {
    assert_refuses_port_held_at("127.0.0.1:0");
}

/// Binding 127.0.0.1 would succeed here; only the occupancy probe notices `localhost` is taken.
#[test]
fn port_held_only_on_ipv6_loopback_fails_with_clear_error() {
    assert_refuses_port_held_at("[::1]:0");
}

#[test]
fn serves_bundled_javascript_license_notice() {
    let server = Server::start();
    let notice = server.get("/.vite/license.md");

    assert_eq!(notice.status, 200);
    assert_eq!(notice.header("content-type"), Some("text/markdown"));
}

#[test]
fn stalled_client_cannot_block_shutdown() {
    let server = Server::start();
    let mut stalled = TcpStream::connect(server.addr).expect("connect");
    // Headers that never finish: graceful shutdown alone would wait on this forever.
    write!(stalled, "GET / HTTP/1.1\r\nHost: {}\r\n", server.addr).expect("write partial request");
    thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    let (status, stderr) = server.terminate();

    assert!(status.success(), "exit status {status}: {stderr}");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "took {:?}",
        started.elapsed()
    );
    assert!(
        stderr.contains("dropping connections still open"),
        "{stderr}"
    );
}

#[test]
fn sigterm_stops_the_server_cleanly() {
    let server = Server::start();
    let addr = server.addr;

    let (status, stderr) = server.terminate();

    assert!(status.success(), "exit status {status}: {stderr}");
    assert!(stderr.contains("stackscan stopped"), "{stderr}");
    assert!(TcpStream::connect(addr).is_err(), "listener still open");
}
