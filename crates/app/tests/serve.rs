//! Drives the compiled `kosse` binary over real TCP: health, the embedded bundle, the
//! occupied-port refusal and bounded shutdown. Each test owns the child it spawns and stops it by
//! that handle only.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use kosse_core::contract::Health;
use support::{OwnedChild, Server};

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
            name: "kosse".to_owned(),
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
    let token = server.token();
    // Past the request policy, a write to a bundle path is simply not a route.
    assert_eq!(server.post_json("/", &token, "{}").status, 405);
    assert_eq!(server.post_json("/api/health", &token, "{}").status, 405);
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

/// The installed default port. If something (even another Kossé) already listens there, it is
/// only connected to, never disturbed; otherwise this test holds it briefly.
#[test]
fn occupied_default_port_fails_with_clear_error() {
    let _held = TcpListener::bind("127.0.0.1:47831");
    let (status, stderr) = OwnedChild::run(&["serve", "--no-open"]);

    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("127.0.0.1:47831 is already in use")
            || stderr.contains("[::1]:47831 is already in use"),
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
    assert!(stderr.contains("kosse stopped"), "{stderr}");
    assert!(TcpStream::connect(addr).is_err(), "listener still open");
}
