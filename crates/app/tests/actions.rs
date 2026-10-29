//! Stop and force through the real binary's HTTP API, against owned fixture processes only.
//! Every server may signal only its own test's fixtures (`--act-only`); refusals of Kossé
//! itself and of other users' processes are asserted by observing that nothing changed, never by
//! sending a signal from the test.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::unix::process::ExitStatusExt,
    process::Command,
    thread,
    time::Duration,
};

use kosse_core::{
    action::{ActionResult, Ineligible, StaleReason},
    model::{ProcessIdentity, Service, Snapshot},
};
use kosse_fixtures::{Fixture, FixtureSpec, live_start_time, lsof};
use support::{Server, eventually};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
const SEEN_WITHIN: Duration = Duration::from_secs(10);
const EXIT_WITHIN: Duration = Duration::from_secs(5);

fn server_acting_on(pids: &[u32]) -> Server {
    let pids: Vec<String> = pids.iter().map(ToString::to_string).collect();
    Server::start_with(&[&format!("--act-only={}", pids.join(","))])
}

/// The first snapshot in which `matches` finds a service, and that service.
fn wait_for(server: &Server, matches: impl Fn(&Service) -> bool) -> (Snapshot, Service) {
    eventually(SEEN_WITHIN, || {
        let snapshot = server.snapshot();
        let service = snapshot
            .observation
            .as_ref()?
            .services
            .iter()
            .find(|service| matches(service))
            .cloned()?;
        Some((snapshot, service))
    })
    .expect("service in a snapshot")
}

fn ports_of(service: &Service) -> Vec<u16> {
    service
        .endpoints
        .iter()
        .map(|endpoint| endpoint.port)
        .collect()
}

fn body(instance: &str, identity: ProcessIdentity, ports: &[u16]) -> String {
    serde_json::json!({ "instance": instance, "identity": identity, "ports": ports }).to_string()
}

/// Posts an action, waiting out the one-per-second limit.
fn act(server: &Server, token: &str, path: &str, body: &str) -> (u16, ActionResult) {
    let response = server.post_json_admitted(path, token, body);
    let result = serde_json::from_slice(&response.body)
        .unwrap_or_else(|_| panic!("{} {}", response.status, response.text()));
    (response.status, result)
}

/// The fixture still runs as the same process and still holds all its ports.
fn assert_untouched(fixture: &Fixture) {
    assert_eq!(
        live_start_time(fixture.pid()),
        Some(fixture.record().started_at_secs),
        "fixture {} changed",
        fixture.pid()
    );
    let listening = lsof::tcp_listeners(&[fixture.pid()]).expect("lsof");
    let ports: std::collections::BTreeSet<u16> =
        listening.iter().map(|&(_, _, port)| port).collect();
    let recorded: std::collections::BTreeSet<u16> = fixture
        .record()
        .endpoints
        .iter()
        .map(std::net::SocketAddr::port)
        .collect();
    assert_eq!(ports, recorded);
}

fn signal_of(fixture: &mut Fixture) -> Option<i32> {
    fixture.wait(EXIT_WITHIN).expect("fixture exited").signal()
}

#[test]
fn term_stops_an_owned_fixture_and_its_ports_are_released() {
    let mut fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).listen(&[V4])).expect("fixture");
    let server = server_acting_on(&[fixture.pid()]);
    let token = server.token();
    let (snapshot, service) = wait_for(&server, |s| s.process.identity.pid == fixture.pid());
    let ports = ports_of(&service);
    assert_eq!(ports.len(), 2);

    let (status, result) = act(
        &server,
        &token,
        "/api/stop",
        &body(&snapshot.instance, service.process.identity, &ports),
    );
    assert_eq!(status, 200, "{result:?}");
    assert_eq!(
        result,
        ActionResult::Exited {
            target: service.process.identity,
            ports: ports.clone(),
            shared: Vec::new(),
        }
    );
    assert_eq!(signal_of(&mut fixture), Some(15), "SIGTERM");
    assert!(
        lsof::tcp_listeners(&[fixture.pid()])
            .expect("lsof")
            .is_empty()
    );
    // The next publication no longer lists it.
    let gone = eventually(SEEN_WITHIN, || {
        let snapshot = server.snapshot();
        let services = &snapshot.observation.as_ref()?.services;
        (!services
            .iter()
            .any(|s| s.process.identity == service.process.identity))
        .then_some(())
    });
    assert!(gone.is_some());
}

#[test]
fn a_confirmation_must_name_the_current_identity_and_every_port() {
    let mut fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).listen(&[V4])).expect("fixture");
    let server = server_acting_on(&[fixture.pid()]);
    let token = server.token();
    let (snapshot, service) = wait_for(&server, |s| s.process.identity.pid == fixture.pid());
    let identity = service.process.identity;
    let ports = ports_of(&service);
    let started = identity.started_at_secs.expect("start time");
    let stale = |reason| (409, ActionResult::Stale { reason });

    let cases = [
        (
            body(&snapshot.instance, identity, &ports[..1]),
            stale(StaleReason::PortsChanged),
        ),
        (
            body(&snapshot.instance, identity, &[ports[0], ports[1], 1]),
            stale(StaleReason::PortsChanged),
        ),
        (
            body(
                &snapshot.instance,
                ProcessIdentity {
                    started_at_secs: Some(started - 1),
                    ..identity
                },
                &ports,
            ),
            stale(StaleReason::IdentityChanged),
        ),
        (
            body(
                &snapshot.instance,
                ProcessIdentity {
                    started_at_secs: None,
                    ..identity
                },
                &ports,
            ),
            (
                403,
                ActionResult::Refused {
                    reason: Ineligible::NoStartTime,
                },
            ),
        ),
        (
            body("an-earlier-instance", identity, &ports),
            stale(StaleReason::InstanceChanged),
        ),
    ];
    for (request, expected) in cases {
        assert_eq!(act(&server, &token, "/api/stop", &request), expected);
        assert_untouched(&fixture);
    }

    fixture.stop().expect("cleanup");
    let (status, result) = act(
        &server,
        &token,
        "/api/stop",
        &body(&snapshot.instance, identity, &ports),
    );
    assert_eq!((status, result), stale(StaleReason::Exited));
}

#[test]
fn a_fixture_ignoring_term_stays_until_an_explicit_force() {
    let mut fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).ignore_term()).expect("fixture");
    let server = server_acting_on(&[fixture.pid()]);
    let token = server.token();
    let (snapshot, service) = wait_for(&server, |s| s.process.identity.pid == fixture.pid());
    let identity = service.process.identity;
    let request = body(&snapshot.instance, identity, &ports_of(&service));

    assert_eq!(
        act(&server, &token, "/api/force", &request),
        (409, ActionResult::ForceNotOffered),
        "force needs a stillRunning result first"
    );
    assert_untouched(&fixture);

    let (status, result) = act(&server, &token, "/api/stop", &request);
    assert_eq!(status, 200);
    assert!(
        matches!(result, ActionResult::StillRunning { target, .. } if target == identity),
        "{result:?}"
    );
    // Nothing escalates by itself.
    thread::sleep(Duration::from_secs(1));
    assert_untouched(&fixture);

    let (status, result) = act(&server, &token, "/api/force", &request);
    assert_eq!(status, 200, "{result:?}");
    assert!(
        matches!(result, ActionResult::Exited { target, .. } if target == identity),
        "{result:?}"
    );
    assert_eq!(signal_of(&mut fixture), Some(9), "SIGKILL");

    // An offer is used once.
    assert_eq!(
        act(&server, &token, "/api/force", &request),
        (409, ActionResult::ForceNotOffered)
    );
}

#[test]
fn a_supervisor_respawn_is_reported_and_the_supervisor_is_left_alone() {
    let supervisor =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).supervise()).expect("supervisor");
    let port = supervisor.record().endpoints[0].port();
    let server = server_acting_on(&[supervisor.pid()]);
    let token = server.token();
    let (snapshot, child) = wait_for(&server, |s| {
        s.process.parent_pid == Some(supervisor.pid()) && ports_of(s) == [port]
    });

    let (status, result) = act(
        &server,
        &token,
        "/api/stop",
        &body(&snapshot.instance, child.process.identity, &[port]),
    );
    assert_eq!(status, 200, "{result:?}");
    let ActionResult::Respawned {
        target,
        parent_pid,
        owners,
        shared,
    } = result
    else {
        panic!("expected respawned, got {result:?}");
    };
    assert_eq!(target, child.process.identity);
    assert_eq!(shared, Vec::new());
    assert_eq!(parent_pid, supervisor.pid());
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].port, port);
    assert_ne!(owners[0].identity.pid, child.process.identity.pid);
    assert_eq!(owners[0].parent_pid, Some(supervisor.pid()));
    assert_eq!(
        live_start_time(child.process.identity.pid),
        None,
        "the child exited"
    );
    assert_eq!(
        live_start_time(supervisor.pid()),
        Some(supervisor.record().started_at_secs),
        "the parent is never signalled"
    );
}

#[test]
fn another_process_taking_the_port_is_not_reported_as_released() {
    let mut fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    let port = fixture.record().endpoints[0].port();
    let server = server_acting_on(&[fixture.pid()]);
    let token = server.token();
    let (snapshot, service) = wait_for(&server, |s| s.process.identity.pid == fixture.pid());
    // An unrelated supervised process waits for the port and binds it as soon as it is free.
    // The guard joins its spawn (bounded by the fixture's ready timeout) even if this test
    // panics, and the supervisor's short lifetime bounds its retry loop.
    let mut successor = Successor(Some(thread::spawn(move || {
        Fixture::spawn(
            &FixtureSpec::new()
                .listen(&[V4])
                .port(port)
                .supervise()
                .max_lifetime(Duration::from_secs(30)),
        )
    })));

    let (status, result) = act(
        &server,
        &token,
        "/api/stop",
        &body(&snapshot.instance, service.process.identity, &[port]),
    );
    let successor = successor.join().expect("successor");
    assert_eq!(status, 200, "{result:?}");
    let ActionResult::EndpointTaken {
        target,
        owners,
        shared,
    } = result
    else {
        panic!("expected endpointTaken, got {result:?}");
    };
    assert_eq!(target, service.process.identity);
    assert_eq!(shared, Vec::new());
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].port, port);
    assert_eq!(owners[0].parent_pid, Some(successor.pid()));
    assert_eq!(signal_of(&mut fixture), Some(15));
}

/// Joins the successor's spawn on drop, so a failing test never leaves it running.
struct Successor(Option<thread::JoinHandle<Result<Fixture, kosse_fixtures::FixtureError>>>);

impl Successor {
    fn join(&mut self) -> Result<Fixture, kosse_fixtures::FixtureError> {
        self.0
            .take()
            .expect("joined once")
            .join()
            .unwrap_or_else(|_| panic!("successor thread panicked"))
    }
}

impl Drop for Successor {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn a_process_already_sharing_the_port_number_is_reported_as_shared() {
    let mut fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    let port = fixture.record().endpoints[0].port();
    // Same port number, other address, unrelated process.
    let neighbour =
        Fixture::spawn(&FixtureSpec::new().listen(&[V6]).port(port)).expect("neighbour");
    let server = server_acting_on(&[fixture.pid()]);
    let token = server.token();
    let (_, neighbour_service) = wait_for(&server, |s| s.process.identity.pid == neighbour.pid());
    let (snapshot, service) = wait_for(&server, |s| s.process.identity.pid == fixture.pid());

    let (status, result) = act(
        &server,
        &token,
        "/api/stop",
        &body(&snapshot.instance, service.process.identity, &[port]),
    );
    assert_eq!(status, 200, "{result:?}");
    let ActionResult::Exited {
        target,
        ports,
        shared,
    } = result
    else {
        panic!("expected exited, got {result:?}");
    };
    assert_eq!(target, service.process.identity);
    assert_eq!(ports, [port]);
    assert_eq!(shared.len(), 1, "{shared:?}");
    assert_eq!(shared[0].port, port);
    assert_eq!(shared[0].identity, neighbour_service.process.identity);
    assert_eq!(signal_of(&mut fixture), Some(15));
    assert_untouched(&neighbour);
}

#[test]
fn act_only_refuses_pid_0_and_1() {
    for pid in ["0", "1", "1,2"] {
        let (status, stderr) = support::OwnedChild::run(&[
            "serve",
            "--no-open",
            "--port",
            "0",
            &format!("--act-only={pid}"),
        ]);
        assert_eq!(status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("can never be signalled"), "{stderr}");
    }
}

/// `ps`'s start time of a running process (independent of Kossé's inspection).
fn ps_start(pid: u32) -> Option<String> {
    let output = Command::new("/bin/ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let start = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !start.is_empty()).then_some(start)
}

/// A running process owned by root other than launchd, found without touching it.
fn root_process() -> u32 {
    let output = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,uid="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let pid: u32 = fields.next()?.parse().ok()?;
            let uid: u32 = fields.next()?.parse().ok()?;
            (uid == 0 && pid > 1).then_some(pid)
        })
        .expect("a root process")
}

#[test]
/// Kossé itself, launchd, a root process (refused as `ownerUnknown`: macOS discloses no owner
/// of root's processes to a user) and an owned process outside the test scope.
fn kosse_itself_system_foreign_and_out_of_scope_processes_are_refused_without_a_signal() {
    let bystander = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    // This server may signal nothing at all.
    let server = Server::start();
    let token = server.token();
    let kosse_pid = server.child.0.id();
    let (snapshot, kosse) = wait_for(&server, |s| s.process.identity.pid == kosse_pid);
    let (_, fixture) = wait_for(&server, |s| s.process.identity.pid == bystander.pid());

    let root_pid = root_process();
    let root_started = ps_start(root_pid).expect("root process start");
    let refused = |reason| (403, ActionResult::Refused { reason });
    let cases = [
        (
            kosse.process.identity,
            ports_of(&kosse),
            refused(Ineligible::KosseItself),
        ),
        (
            ProcessIdentity {
                pid: 1,
                started_at_secs: Some(1),
            },
            vec![1],
            refused(Ineligible::SystemProcess),
        ),
        (
            // macOS discloses neither owner nor start time of root's processes to a user.
            ProcessIdentity {
                pid: root_pid,
                started_at_secs: Some(1),
            },
            vec![1],
            refused(Ineligible::OwnerUnknown),
        ),
        (
            fixture.process.identity,
            ports_of(&fixture),
            refused(Ineligible::OutsideTestScope),
        ),
    ];
    for path in ["/api/stop", "/api/force"] {
        for (identity, ports, expected) in &cases {
            let request = body(&snapshot.instance, *identity, ports);
            let outcome = act(&server, &token, path, &request);
            // Force is refused even earlier: there is no stillRunning offer for these.
            if path == "/api/force" {
                assert_eq!(outcome, (409, ActionResult::ForceNotOffered));
            } else {
                assert_eq!(&outcome, expected, "{identity:?}");
            }
        }
    }
    assert_eq!(server.get("/api/health").status, 200, "Kossé still serves");
    assert_eq!(
        ps_start(root_pid),
        Some(root_started),
        "the root process still runs"
    );
    assert_untouched(&bystander);
}

#[test]
fn actions_need_the_token_and_are_rate_limited() {
    let server = Server::start();
    let token = server.token();
    let snapshot = server.snapshot();
    let request = body(
        &snapshot.instance,
        ProcessIdentity {
            pid: 1,
            started_at_secs: Some(0),
        },
        &[1],
    );
    for path in ["/api/stop", "/api/force"] {
        let response = server.post_json(path, "not-the-token", &request);
        assert_eq!(response.status, 403);
        assert_eq!(response.error_code(), "tokenRequired");
        let response = server.post_json(path, &token, "{\"identity\":1}");
        assert_eq!(response.status, 400);
        assert_eq!(response.error_code(), "invalidPayload");
        let response = server.get(path);
        assert_eq!(response.status, 405, "never a destructive GET");
    }
    let first = server.post_json("/api/stop", &token, &request);
    assert_eq!(first.status, 403, "{}", first.text());
    let second = server.post_json("/api/force", &token, &request);
    assert_eq!(second.status, 429);
    assert_eq!(second.error_code(), "rateLimited");
    assert_eq!(second.header("retry-after"), Some("1"));
}
