//! Fake secrets in an owned fixture's command line never leave Kossé raw: not through
//! `list --json`, the table, `/api/snapshot`, the event stream, or the server's stderr.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    net::{IpAddr, Ipv4Addr},
    process::Command,
    time::Duration,
};

use kosse_core::model::{CommandLine, Snapshot};
use kosse_fixtures::{Fixture, FixtureSpec};
use support::{EventStream, Server};

const MARKUP: &str = "<img src=x onerror=alert(1)>";

/// Credential-shaped values, assembled at run time so the source holds no scanner-flagged
/// literal.
fn secrets() -> Vec<String> {
    vec![
        format!("gh{}_{}", "p", "FAKE0fixture0token0value0abcdefghij"),
        format!("xox{}-{}", "b", "1111-2222-fixturefake"),
        format!("AKI{}{}", "A", "FIXTUREFAKE00000"),
        format!(
            "ey{}.{}.{}",
            "JhbGciOiJIUzI1NiJ9", "eyJzdWIiOiJmaXh0dXJlIn0", "c2lnbmF0dXJl"
        ),
        "hunter2-fixture-password".to_owned(),
        "pa55-in-url".to_owned(),
        "env-style-secret".to_owned(),
        "bearer-fixture-credential".to_owned(),
        "json-fixture-api-key".to_owned(),
        "user-flag-fixture-password".to_owned(),
    ]
}

fn fixture() -> Fixture {
    let s = secrets();
    let args = [
        format!("--token={}", s[0]),
        format!("--slack={},{}", s[1], s[2]),
        format!("--id-token={}", s[3]),
        "--password".to_owned(),
        s[4].clone(),
        format!("postgres://app:{}@127.0.0.1:5432/db", s[5]),
        format!("API_SECRET={}", s[6]),
        format!("Authorization: Bearer {}", s[7]),
        format!(r#"--config={{"apiKey":"{}","port":1}}"#, s[8]),
        "-u".to_owned(),
        format!("me:{}", s[9]),
        MARKUP.to_owned(),
    ];
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[IpAddr::V4(Ipv4Addr::LOCALHOST)])
            .args(&args),
    )
    .expect("fixture")
}

fn assert_clean(what: &str, text: &str) {
    for secret in secrets() {
        assert!(!text.contains(&secret), "{what} leaked a fixture secret");
    }
    for fragment in ["pa55-in-url", "json-fixture", "user-flag-fixture"] {
        assert!(
            !text.contains(fragment),
            "{what} leaked part of a fixture secret"
        );
    }
}

fn command_of(snapshot: &Snapshot, fixture: &Fixture) -> CommandLine {
    snapshot
        .observation
        .as_ref()
        .expect("observation")
        .services
        .iter()
        .find(|service| service.process.identity.pid == fixture.pid())
        .unwrap_or_else(|| panic!("fixture {} missing", fixture.pid()))
        .process
        .command
        .clone()
        .expect("own process's command line is readable")
}

#[test]
fn command_lines_are_redacted_on_every_output() {
    let fixture = fixture();

    let list = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .args(["list", "--json", "--port", "0"])
        .output()
        .expect("run list");
    assert!(list.status.success());
    let json = String::from_utf8(list.stdout).expect("utf-8");
    assert_clean("list --json", &json);
    assert_clean("list stderr", &String::from_utf8_lossy(&list.stderr));
    let snapshot: Snapshot = serde_json::from_str(&json).expect("snapshot");
    let command = command_of(&snapshot, &fixture);
    assert!(command.redacted && !command.truncated);
    let tail: Vec<&str> = command
        .args
        .iter()
        .map(String::as_str)
        .skip_while(|a| *a != "--")
        .collect();
    assert_eq!(
        tail,
        [
            "--",
            "--token=[redacted]",
            "--slack=[redacted],[redacted]",
            "--id-token=[redacted]",
            "--password",
            "[redacted]",
            "postgres://[redacted]@127.0.0.1:5432/db",
            "API_SECRET=[redacted]",
            "Authorization: [redacted]",
            r#"--config={"apiKey":"[redacted]","port":1}"#,
            "-u",
            "me:[redacted]",
            // Markup is data: kept verbatim for the UI to escape, served as JSON (nosniff).
            MARKUP,
        ]
    );

    let table = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .args(["list", "--port", "0"])
        .output()
        .expect("run list");
    assert_clean("list table", &String::from_utf8_lossy(&table.stdout));

    let server = Server::start();
    let response = server.get("/api/snapshot");
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert_clean("/api/snapshot", &response.text());
    let published: Snapshot = serde_json::from_slice(&response.body).expect("snapshot");
    assert_eq!(command_of(&published, &fixture), command);

    let mut events = EventStream::open(&server);
    let pushed = events.next_snapshot(Duration::from_secs(5)).expect("event");
    assert_eq!(command_of(&pushed, &fixture), command);
    assert_clean("/api/events", &events.raw_text());

    let (status, stderr) = server.terminate();
    assert!(status.success());
    assert_clean("serve stderr", &stderr);
}
