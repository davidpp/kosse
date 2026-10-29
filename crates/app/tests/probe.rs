//! HTTP enrichment through the real binary: owned fixtures in every `--serve` mode, observed via
//! `/api/snapshot` of a running `kosse serve`. Each server probes only its own test's fixtures
//! (`--probe-only`), so no real service is ever probed. Counts are exact per server: they include
//! only requests that carry its instance ID, so a developer's own running Kossé, which probes the
//! fixture projects too, does not disturb them.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
    time::Duration,
};

use kosse_core::model::{HttpProbe, ProbeResult, ProbeSkip, Snapshot, TlsFailure};
use kosse_fixtures::{
    Fixture, FixtureSpec,
    protocol::{REDIRECT_PATH, Serve},
    repo::Sandbox,
};
use support::{RawRequest, Server, eventually, sent_by};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
/// First scan, the probes (a hanging one takes its full head timeout), the next scan.
const PROBED_WITHIN: Duration = Duration::from_secs(12);

/// A project directory (a `package.json` outside Git): only processes in a project are probed
/// automatically.
fn project() -> (Sandbox, PathBuf) {
    let sandbox = Sandbox::new().expect("sandbox");
    let manifest = sandbox.file("app/package.json", "{}").expect("manifest");
    let dir = manifest.parent().expect("dir").to_path_buf();
    (sandbox, dir)
}

fn spawn_in(dir: &std::path::Path, serve: Serve, addresses: &[IpAddr]) -> Fixture {
    Fixture::spawn(
        &FixtureSpec::new()
            .listen(addresses)
            .serve(serve)
            .record()
            .cwd(dir),
    )
    .expect("fixture")
}

/// A server that probes exactly these fixtures.
fn server_for(fixtures: &[&Fixture]) -> Server {
    let pids: Vec<String> = fixtures.iter().map(|f| f.pid().to_string()).collect();
    Server::start_with(&[&format!("--probe-only={}", pids.join(","))])
}

fn probe_of(snapshot: &Snapshot, fixture: &Fixture) -> HttpProbe {
    let observation = snapshot.observation.as_ref().expect("observation");
    let service = observation
        .services
        .iter()
        .find(|service| service.process.identity.pid == fixture.pid())
        .unwrap_or_else(|| panic!("fixture {} missing", fixture.pid()));
    assert_eq!(service.endpoints.len(), 1);
    service.endpoints[0].http.clone()
}

/// Waits until every fixture has a probe result, then returns them in order.
fn results(server: &Server, fixtures: &[&Fixture]) -> Vec<(u64, ProbeResult)> {
    eventually(PROBED_WITHIN, || {
        let snapshot = server.snapshot();
        fixtures
            .iter()
            .map(|fixture| match probe_of(&snapshot, fixture) {
                HttpProbe::Probed {
                    probed_at_ms,
                    requested: false,
                    result,
                } => Some((probed_at_ms, result)),
                _ => None,
            })
            .collect()
    })
    .unwrap_or_else(|| panic!("not every fixture was probed within {PROBED_WITHIN:?}"))
}

/// The requests `fixture` received from `server`.
fn requests(server: &Server, fixture: &Fixture) -> Vec<String> {
    sent_by(fixture.received(port(fixture)), &server.snapshot().instance)
}

fn port(fixture: &Fixture) -> u16 {
    fixture.record().endpoints[0].port()
}

#[test]
fn responses_of_any_status_are_reachable_http_with_loopback_urls() {
    let (_sandbox, dir) = project();
    let title = spawn_in(&dir, Serve::Title, &[V4]);
    let not_found = spawn_in(&dir, Serve::NotFound, &[V4]);
    let error = spawn_in(&dir, Serve::Error, &[V4]);
    let ipv6_only = spawn_in(&dir, Serve::Title, &[V6]);
    let server = server_for(&[&title, &not_found, &error, &ipv6_only]);

    let found = results(&server, &[&title, &not_found, &error, &ipv6_only]);
    for ((_, result), (fixture, status)) in found.iter().zip([
        (&title, 200),
        (&not_found, 404),
        (&error, 500),
        (&ipv6_only, 200),
    ]) {
        let ProbeResult::Response {
            url,
            status: seen,
            tls,
            location,
            body_truncated,
            ..
        } = result
        else {
            panic!("fixture {} not reachable: {result:?}", fixture.pid());
        };
        assert_eq!(*seen, status);
        assert_eq!(url, &format!("http://localhost:{}/", port(fixture)));
        assert!(!tls && location.is_none() && !body_truncated);
        // One probe, one plain GET: no TLS retry for an HTTP answer.
        assert_eq!(requests(&server, fixture).len(), 1, "{}", fixture.pid());
    }
    let ProbeResult::Response { title: page, .. } = &found[0].1 else {
        unreachable!()
    };
    assert_eq!(page.as_deref(), Some("Fixture & <Title>"));
    let ProbeResult::Response { title: json, .. } = &found[1].1 else {
        unreachable!()
    };
    assert_eq!(json, &None, "a JSON 404 has no title");

    // The plain GET carries the loopback Host, the instance's opaque snapshot ID and nothing
    // that identifies the user.
    let request = requests(&server, &title).remove(0).to_ascii_lowercase();
    assert!(request.starts_with("get / http/1.1\r\n"), "{request}");
    assert!(
        request.contains(&format!("host: localhost:{}\r\n", port(&title))),
        "{request}"
    );
    assert!(
        !request.contains("cookie") && !request.contains("authorization"),
        "{request}"
    );
}

#[test]
fn hanging_huge_and_redirecting_endpoints_are_bounded() {
    let (_sandbox, dir) = project();
    let hang = spawn_in(&dir, Serve::Hang, &[V4]);
    let huge = spawn_in(&dir, Serve::Huge, &[V4]);
    let redirect = spawn_in(&dir, Serve::Redirect, &[V4]);
    let server = server_for(&[&hang, &huge, &redirect]);

    let found = results(&server, &[&hang, &huge, &redirect]);
    assert_eq!(found[0].1, ProbeResult::TimedOut);
    let ProbeResult::Response {
        status,
        title,
        body_truncated,
        ..
    } = &found[1].1
    else {
        panic!("huge: {:?}", found[1].1);
    };
    assert_eq!(
        (*status, title.as_deref(), *body_truncated),
        (200, Some("huge"), true)
    );
    let ProbeResult::Response {
        status, location, ..
    } = &found[2].1
    else {
        panic!("redirect: {:?}", found[2].1);
    };
    assert_eq!(*status, 302);
    let location = location.as_deref().expect("location recorded");
    assert_eq!(
        location,
        format!("http://localhost:{}/next?token=[redacted]", port(&redirect))
    );
    assert!(!location.contains("fixture-redirect-secret"));
    // Recorded, not followed: the target is the fixture itself, which saw exactly one request,
    // `GET /`, and never `GET /next`.
    let sent = requests(&server, &redirect);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("GET / HTTP/1.1\r\n"), "{sent:?}");
    assert!(!sent[0].contains(REDIRECT_PATH));
    for fixture in [&hang, &huge] {
        assert_eq!(requests(&server, fixture).len(), 1);
    }
    // Probing is cached: a later snapshot reuses the same results.
    let later = server.snapshot();
    assert_eq!(
        probe_of(&later, &redirect),
        HttpProbe::Probed {
            probed_at_ms: found[2].0,
            requested: false,
            result: found[2].1.clone()
        }
    );
}

#[test]
fn self_signed_tls_is_a_certificate_failure_and_banners_get_no_request() {
    let (_sandbox, dir) = project();
    let tls = spawn_in(&dir, Serve::Tls, &[V4]);
    let banner = spawn_in(&dir, Serve::Banner, &[V4]);
    let server = server_for(&[&tls, &banner]);

    let found = results(&server, &[&tls, &banner]);
    let ProbeResult::TlsFailed { failure, reason } = &found[0].1 else {
        panic!("self-signed TLS: {:?}", found[0].1);
    };
    assert_eq!(*failure, TlsFailure::Certificate, "{reason}");
    assert!(reason.contains("certificate"), "{reason}");
    assert_eq!(found[1].1, ProbeResult::NotHttp);
    // The banner server spoke first: connected to, closed without writing anything. A silent
    // connection carries no instance ID, so another running Kossé's may be among them.
    let connections = banner.received(port(&banner));
    assert!(
        !connections.is_empty() && connections.iter().all(String::is_empty),
        "{connections:?}"
    );
}

#[test]
fn only_attributed_processes_are_probed_automatically_and_try_http_is_explicit() {
    let (_sandbox, dir) = project();
    let attributed = spawn_in(&dir, Serve::Title, &[V4]);
    // A fresh temporary directory: no manifest, no Git, so no project.
    let loose = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .serve(Serve::Title)
            .record(),
    )
    .expect("fixture");
    let server = server_for(&[&attributed, &loose]);

    results(&server, &[&attributed]);
    let snapshot = server.snapshot();
    assert_eq!(
        probe_of(&snapshot, &loose),
        HttpProbe::Skipped {
            reason: ProbeSkip::NotAttributed
        }
    );
    assert_eq!(
        requests(&server, &loose).len(),
        0,
        "never probed on its own"
    );

    // "Try HTTP": an explicit, guarded, rate-limited request.
    let token = server.token();
    let pid = loose.pid();
    let body = |started: u64| {
        format!(
            r#"{{"identity":{{"pid":{pid},"startedAtSecs":{started}}},"port":{}}}"#,
            port(&loose)
        )
    };
    let started = loose.record().started_at_secs;
    let response = server.post_json("/api/probe", &token, &body(started));
    assert_eq!(response.status, 200, "{}", response.text());
    let probed: HttpProbe = serde_json::from_slice(&response.body).expect("probe json");
    let HttpProbe::Probed {
        requested: true,
        result: ProbeResult::Response { status: 200, .. },
        ..
    } = probed
    else {
        panic!("requested probe: {probed:?}");
    };
    assert_eq!(requests(&server, &loose).len(), 1);
    let limited = server.post_json("/api/probe", &token, &body(started));
    assert_eq!(limited.status, 429);
    assert_eq!(limited.error_code(), "rateLimited");

    // The next snapshot carries it, labelled as requested.
    let shown = eventually(Duration::from_secs(5), || {
        match probe_of(&server.snapshot(), &loose) {
            shown @ HttpProbe::Probed { .. } => Some(shown),
            _ => None,
        }
    })
    .expect("requested result in a snapshot");
    assert_eq!(shown, probed);

    // A recycled or ended identity is a conflict; malformed requests are refused.
    std::thread::sleep(Duration::from_millis(1100));
    let stale = server.post_json("/api/probe", &token, &body(started - 1));
    assert_eq!(
        (stale.status, stale.error_code().as_str()),
        (409, "staleIdentity")
    );
    for invalid in ["{}", r#"{"identity":{"pid":1},"port":"x"}"#] {
        let response = server.post_json("/api/probe", &token, invalid);
        assert_eq!(response.error_code(), "invalidPayload", "{invalid}");
    }
    let foreign = server.send(
        &RawRequest::new("POST", "/api/probe")
            .host(&server.authority())
            .header("Origin", "https://evil.example")
            .header("Content-Type", "application/json")
            .header("X-Kosse-Token", &token)
            .body(&body(started)),
    );
    assert_eq!(foreign.error_code(), "originNotAllowed");
    assert_eq!(
        requests(&server, &loose).len(),
        1,
        "refused requests probe nothing"
    );
}

#[test]
fn a_one_shot_list_reports_candidates_as_pending() {
    let (_sandbox, dir) = project();
    let title = spawn_in(&dir, Serve::Title, &[V4]);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .args(["list", "--json", "--port", "0"])
        .output()
        .expect("run list");
    assert!(output.status.success());
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout).expect("snapshot");
    assert_eq!(probe_of(&snapshot, &title), HttpProbe::Pending);
    assert_eq!(
        sent_by(title.received(port(&title)), &snapshot.instance).len(),
        0,
        "list never probes"
    );
}
