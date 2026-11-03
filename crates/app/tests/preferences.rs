//! Dashboard preferences through the real binary: `kosse serve` with a temporary configuration
//! directory (`KOSSE_CONFIG_DIR`), never the user's own.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{fs, os::unix::fs::PermissionsExt};

use kosse_core::contract::Preferences;
use kosse_fixtures::repo::Sandbox;
use serde_json::{Value, json};
use support::{RawRequest, Server};

fn preferences(server: &Server) -> Preferences {
    let response = server.get("/api/preferences");
    assert_eq!(response.status, 200, "{}", response.text());
    serde_json::from_slice(&response.body).expect("preferences json")
}

fn post(server: &Server, token: &str, body: &str) -> support::HttpResponse {
    server.post_json("/api/preferences", token, body)
}

#[test]
fn preferences_persist_across_restarts_and_keep_the_rest_of_the_file() {
    let tree = Sandbox::new().expect("sandbox");
    let config = tree.root().join("config");
    let file = config.join("config.json");

    let server = Server::start_configured(&config, &[]);
    assert_eq!(preferences(&server), Preferences::default());
    assert!(!config.exists(), "reading the defaults creates nothing");

    // The same guard as every unsafe request, before anything is written.
    let token = server.token();
    let body = r#"{"allListeners":true}"#;
    let refusals = [
        (
            RawRequest::new("POST", "/api/preferences")
                .host(&server.authority())
                .header("Origin", &server.origin())
                .header("Content-Type", "application/json")
                .body(body),
            403,
            "tokenRequired",
        ),
        (
            RawRequest::new("POST", "/api/preferences")
                .host(&server.authority())
                .header("Origin", "http://evil.example")
                .header("Content-Type", "application/json")
                .header("X-Kosse-Token", &token)
                .body(body),
            403,
            "originNotAllowed",
        ),
        (
            RawRequest::new("POST", "/api/preferences")
                .host(&server.authority())
                .header("Origin", &server.origin())
                .header("Content-Type", "text/plain")
                .header("X-Kosse-Token", &token)
                .body(body),
            415,
            "unsupportedMediaType",
        ),
    ];
    for (request, status, code) in refusals {
        let refused = server.send(&request);
        assert_eq!(refused.status, status, "{}", refused.text());
        assert_eq!(refused.error_code(), code);
    }
    for invalid in [
        r#"{"allListeners":"yes"}"#,
        r#"{"allListeners":true,"theme":"x"}"#,
    ] {
        let refused = post(&server, &token, invalid);
        assert_eq!(refused.status, 400, "{}", refused.text());
        assert_eq!(refused.error_code(), "invalidPayload");
    }
    assert!(!file.exists(), "a refused request writes nothing");

    // A file written by hand (or a later Kossé) keeps its other parts.
    fs::create_dir_all(&config).expect("dir");
    fs::write(
        &file,
        r#"{"version":1,"later":1,"preferences":{"theme":"dark"},"projects":{"/work/app":{"version":1,"name":"App"}}}"#,
    )
    .expect("write");
    let saved = post(&server, &token, body);
    assert_eq!(saved.status, 200, "{}", saved.text());
    let stored: Preferences = serde_json::from_slice(&saved.body).expect("json");
    assert!(stored.all_listeners);
    let limited = post(&server, &token, r#"{"allListeners":false}"#);
    assert_eq!(limited.status, 429, "{}", limited.text());
    assert_eq!(limited.header("retry-after"), Some("1"));

    let written: Value = serde_json::from_slice(&fs::read(&file).expect("read")).expect("json");
    assert_eq!(
        written,
        json!({
            "version": 1,
            "later": 1,
            "preferences": { "allListeners": true, "theme": "dark" },
            "projects": { "/work/app": { "version": 1, "name": "App" } }
        })
    );
    let mode = fs::metadata(&file).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let (status, _) = server.terminate();
    assert!(status.success());

    let restarted = Server::start_configured(&config, &[]);
    assert!(
        preferences(&restarted).all_listeners,
        "reloaded after a restart"
    );
    let token = restarted.token();
    let saved = post(&restarted, &token, r#"{"allListeners":false}"#);
    assert_eq!(saved.status, 200, "{}", saved.text());
    assert_eq!(preferences(&restarted), Preferences::default());
    let (status, _) = restarted.terminate();
    assert!(status.success());
}

#[test]
fn unusable_configuration_is_reported_and_never_overwritten() {
    let tree = Sandbox::new().expect("sandbox");
    let config = tree.root().join("config");
    let file = config.join("config.json");
    fs::create_dir_all(&config).expect("dir");
    let server = Server::start_configured(&config, &[]);
    let token = server.token();
    for contents in [
        "{\"version\": 1, \"preferences\": {",
        r#"{"version": 1, "preferences": {"allListeners": "yes"}}"#,
        r#"{"version": 9, "preferences": {"allListeners": true, "density": "compact"}}"#,
    ] {
        fs::write(&file, contents).expect("write");
        let read = server.get("/api/preferences");
        assert_eq!(read.status, 409, "{}", read.text());
        assert_eq!(read.error_code(), "configUnavailable");
        let refused =
            server.post_json_admitted("/api/preferences", &token, r#"{"allListeners":true}"#);
        assert_eq!(refused.status, 409, "{}", refused.text());
        assert_eq!(refused.error_code(), "configUnavailable");
        assert!(refused.text().contains("config.json"), "{}", refused.text());
        assert_eq!(fs::read_to_string(&file).expect("read"), contents);
    }
    let (status, _) = server.terminate();
    assert!(status.success());
}
