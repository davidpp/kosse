//! `kosse stop` and `kosse force` through the compiled binary, as a client of a running
//! `kosse serve` that may signal only the test's fixtures (`--act-only`). Expected identities and
//! ports come from the fixtures' own records and `lsof`; refusals are asserted by observing that
//! nothing changed.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, TcpListener},
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use kosse_core::{
    action::{ActionResult, Ineligible, StaleReason},
    agent::{AGENT_SCHEMA_VERSION, ActionAnswer, CommandError, CommandErrorCode},
    model::{ProcessIdentity, Service},
    profiles::view::ProfileMatch,
};
use kosse_fixtures::{
    Fixture, FixtureSpec, live_start_time, lsof,
    repo::{Sandbox, git, init_repo},
};
use support::{NO_CONFIG, Server, eventually};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const SEEN_WITHIN: Duration = Duration::from_secs(10);
const EXIT_WITHIN: Duration = Duration::from_secs(5);
/// The instance accepts one action per second; a refused request counts too.
const RATE_LIMITED_FOR: Duration = Duration::from_secs(5);

fn server_acting_on(fixtures: &[&Fixture]) -> Server {
    let pids: Vec<String> = fixtures.iter().map(|f| f.pid().to_string()).collect();
    Server::start_with(&[&format!("--act-only={}", pids.join(","))])
}

/// Runs `kosse <args>` in `dir`, retrying while the instance's one-action-per-second limit
/// refuses it (exit 3, `rateLimited`, nothing signalled).
fn run(dir: &Path, args: &[&str]) -> Output {
    let deadline = Instant::now() + RATE_LIMITED_FOR;
    loop {
        let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .current_dir(dir)
            .args(args)
            .output()
            .expect("run kosse");
        let limited = output.status.code() == Some(3)
            && String::from_utf8_lossy(&output.stderr).contains("retry in a second");
        if !limited || Instant::now() >= deadline {
            return output;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// `kosse <args> --instance-port <server>`.
fn kosse(dir: &Path, server: &Server, args: &[&str]) -> Output {
    let port = server.addr.port().to_string();
    run(dir, &[args, &["--instance-port", &port]].concat())
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn error_of(output: &Output) -> CommandError {
    assert!(output.stdout.is_empty(), "stdout carries only results");
    serde_json::from_slice(&output.stderr)
        .unwrap_or_else(|_| panic!("not a CommandError: {}", stderr(output)))
}

fn answer_of(output: &Output) -> ActionAnswer {
    let answer: ActionAnswer = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("not an ActionAnswer: {} {}", stdout(output), stderr(output)));
    assert_eq!(answer.schema_version, AGENT_SCHEMA_VERSION);
    answer
}

fn identity(fixture: &Fixture) -> ProcessIdentity {
    ProcessIdentity {
        pid: fixture.pid(),
        started_at_secs: Some(fixture.record().started_at_secs),
    }
}

/// The ports `lsof` sees the fixture listen on, ordered.
fn live_ports(fixture: &Fixture) -> Vec<u16> {
    let ports: BTreeSet<u16> = lsof::tcp_listeners(&[fixture.pid()])
        .expect("lsof")
        .into_iter()
        .map(|(_, _, port)| port)
        .collect();
    ports.into_iter().collect()
}

fn joined(ports: &[u16]) -> String {
    let ports: Vec<String> = ports.iter().map(ToString::to_string).collect();
    ports.join(",")
}

/// The confirming command the CLI must print for `fixture`, from its record and `lsof`.
fn confirming(verb: &str, fixture: &Fixture, server: &Server) -> String {
    format!(
        "kosse {verb} {}.{} --ports {} --instance-port {}",
        fixture.pid(),
        fixture.record().started_at_secs,
        joined(&live_ports(fixture)),
        server.addr.port()
    )
}

/// Runs a printed `kosse …` command line (it holds only numbers and flags).
fn run_printed(dir: &Path, command: &str) -> Output {
    let args: Vec<&str> = command
        .strip_prefix("kosse ")
        .expect("a kosse command")
        .split(' ')
        .collect();
    run(dir, &args)
}

/// Waits until the instance's snapshot holds a service that `matches`.
fn observed(server: &Server, matches: impl Fn(&Service) -> bool) -> Service {
    eventually(SEEN_WITHIN, || {
        server
            .snapshot()
            .observation?
            .services
            .into_iter()
            .find(|service| matches(service))
    })
    .expect("service in the instance's snapshot")
}

fn assert_untouched(fixture: &Fixture, ports: &[u16]) {
    assert_eq!(
        live_start_time(fixture.pid()),
        Some(fixture.record().started_at_secs),
        "fixture {} changed",
        fixture.pid()
    );
    assert_eq!(
        live_ports(fixture),
        ports,
        "fixture {} ports",
        fixture.pid()
    );
}

const REPO_PROFILE: &str =
    r#"{ "version": 1, "services": { "web": { "match": { "cwd": "." } } } }"#;

/// The agent's safe two-step: a service name, a port, or an identity without its ports signals
/// nothing and prints the one exact confirming command; that command stops the process.
#[test]
fn a_name_is_refused_with_the_command_that_then_stops_it() {
    let tree = Sandbox::new().expect("sandbox");
    let shop = tree.dir("shop").expect("dir");
    init_repo(&shop).expect("repo");
    tree.file("shop/kosse.json", REPO_PROFILE).expect("file");
    git(&shop, &["add", "-A"]).expect("add");
    git(&shop, &["commit", "-q", "-m", "profile"]).expect("commit");
    let shop = shop.canonicalize().expect("canonical");
    let mut fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4]).listen(&[V4]).cwd(&shop))
        .expect("fixture");
    let ports = live_ports(&fixture);
    assert_eq!(ports.len(), 2);
    let server = server_acting_on(&[&fixture]);
    observed(&server, |service| {
        service.process.identity == identity(&fixture)
            && service.profile
                == Some(ProfileMatch::Matched {
                    service: "web".to_owned(),
                })
    });
    let expected = confirming("stop", &fixture, &server);
    let shop_text = shop.to_str().expect("UTF-8 path").to_owned();

    let first_port = ports[0].to_string();
    let pid = fixture.pid().to_string();
    let pid_start = format!("{pid}.{}", fixture.record().started_at_secs);
    for args in [
        vec!["stop", "web"],
        vec!["stop", "web", "--here"],
        vec!["stop", first_port.as_str()],
        vec!["stop", pid.as_str(), "--ports", first_port.as_str()],
        vec!["stop", pid_start.as_str()],
    ] {
        let output = kosse(&shop, &server, &args);
        assert_eq!(
            output.status.code(),
            Some(3),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "nothing on stdout: {args:?}");
        assert!(
            stderr(&output).contains(&format!("\n  {expected}  (")),
            "{args:?} must print {expected:?}: {}",
            stderr(&output)
        );
        assert_untouched(&fixture, &ports);
    }
    // Other tests run fixtures with a "web" service too: scope the name to this project.
    let refused = kosse(
        tree.root(),
        &server,
        &["stop", "web", "--project", &shop_text, "--json"],
    );
    assert_eq!(refused.status.code(), Some(3));
    let error = error_of(&refused);
    assert_eq!(error.error, CommandErrorCode::ConfirmationRequired);
    assert_eq!(error.confirm.len(), 1, "{error:?}");
    assert_eq!(error.confirm[0].command, expected);
    assert_eq!(error.confirm[0].identity, identity(&fixture));
    assert_eq!(error.confirm[0].ports, ports);
    assert_eq!(
        error.confirm[0].project.as_deref(),
        Some(shop_text.as_str())
    );

    let stopped = run_printed(&shop, &expected);
    assert_eq!(stopped.status.code(), Some(0), "{}", stderr(&stopped));
    let text = stdout(&stopped);
    assert!(text.starts_with("outcome: exited\n"), "{text}");
    assert!(
        text.contains(&format!("ports: {} (free)", ports_text(&ports))),
        "{text}"
    );
    let status = fixture.wait(EXIT_WITHIN).expect("fixture exited");
    assert_eq!(status.signal(), Some(15), "SIGTERM");
    assert!(live_ports(&fixture).is_empty(), "its ports are free");
}

fn ports_text(ports: &[u16]) -> String {
    let ports: Vec<String> = ports.iter().map(ToString::to_string).collect();
    ports.join(", ")
}

/// Force exists only within a stillRunning offer from a stop on the same instance, and is
/// never sent by the stop itself.
#[test]
fn a_term_ignoring_fixture_is_forced_only_after_a_still_running_stop() {
    let mut fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).ignore_term()).expect("fixture");
    let ports = live_ports(&fixture);
    let server = server_acting_on(&[&fixture]);
    observed(&server, |service| {
        service.process.identity == identity(&fixture)
    });
    let dir = Path::new(ANYWHERE);
    let id = format!("{}.{}", fixture.pid(), fixture.record().started_at_secs);
    let port_list = joined(&ports);
    let confirmed = ["--ports", port_list.as_str(), "--json"];

    let early = kosse(
        dir,
        &server,
        &[&["force", id.as_str()][..], &confirmed].concat(),
    );
    assert_eq!(early.status.code(), Some(3), "{}", stderr(&early));
    let error = error_of(&early);
    assert_eq!(error.error, CommandErrorCode::ForceNotOffered);
    assert_eq!(error.result, Some(ActionResult::ForceNotOffered));
    assert_eq!(
        error
            .confirm
            .iter()
            .map(|c| c.command.as_str())
            .collect::<Vec<_>>(),
        [confirming("stop", &fixture, &server)],
        "the refusal names the stop to run first"
    );
    assert_untouched(&fixture, &ports);

    let stop = kosse(
        dir,
        &server,
        &[&["stop", id.as_str()][..], &confirmed].concat(),
    );
    assert_eq!(stop.status.code(), Some(1), "{}", stderr(&stop));
    let answer = answer_of(&stop);
    assert!(
        matches!(
            &answer.result,
            ActionResult::StillRunning { target, ports: p, force_until_ms: Some(_) }
                if *target == identity(&fixture) && *p == ports
        ),
        "{answer:?}"
    );
    let force = answer.force_command.expect("force offered");
    assert_eq!(force, confirming("force", &fixture, &server));
    assert_untouched(&fixture, &ports);

    let forced = run_printed(dir, &format!("{force} --json"));
    assert_eq!(forced.status.code(), Some(0), "{}", stderr(&forced));
    assert!(matches!(
        answer_of(&forced).result,
        ActionResult::Exited { target, .. } if target == identity(&fixture)
    ));
    let status = fixture.wait(EXIT_WITHIN).expect("fixture exited");
    assert_eq!(status.signal(), Some(9), "SIGKILL");
}

/// Where commands that need no project scope run.
const ANYWHERE: &str = "/";

/// A port set that is not every live port, or an identity whose process was replaced, signals
/// nothing; the refusal names what runs now.
#[test]
fn a_stale_identity_or_port_set_is_refused_with_the_current_command() {
    let mut first =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).listen(&[V4])).expect("fixture");
    let ports = live_ports(&first);
    let server = server_acting_on(&[&first]);
    observed(&server, |service| {
        service.process.identity == identity(&first)
    });
    let dir = Path::new(ANYWHERE);
    let old = format!("{}.{}", first.pid(), first.record().started_at_secs);
    let one_port = ports[0].to_string();

    let partial = kosse(
        dir,
        &server,
        &["stop", &old, "--ports", &one_port, "--json"],
    );
    assert_eq!(partial.status.code(), Some(3), "{}", stderr(&partial));
    let error = error_of(&partial);
    assert_eq!(error.error, CommandErrorCode::Stale);
    assert_eq!(
        error.result,
        Some(ActionResult::Stale {
            reason: StaleReason::PortsChanged
        })
    );
    assert_eq!(
        error
            .confirm
            .iter()
            .map(|c| c.command.as_str())
            .collect::<Vec<_>>(),
        [confirming("stop", &first, &server)]
    );
    assert_untouched(&first, &ports);

    // The test restarts its own fixture on the same port: the old identity is stale.
    first.stop().expect("cleanup");
    let second = Fixture::spawn(&FixtureSpec::new().listen(&[V4]).port(ports[0]))
        .expect("restarted fixture");
    let second_ports = live_ports(&second);
    observed(&server, |service| {
        service.process.identity == identity(&second)
    });
    let restarted = kosse(
        dir,
        &server,
        &["stop", &old, "--ports", &joined(&ports), "--json"],
    );
    assert_eq!(restarted.status.code(), Some(3), "{}", stderr(&restarted));
    let error = error_of(&restarted);
    assert_eq!(error.error, CommandErrorCode::Stale);
    assert!(
        matches!(error.result, Some(ActionResult::Stale { .. })),
        "{error:?}"
    );
    assert!(
        error
            .confirm
            .iter()
            .any(|c| c.command == confirming("stop", &second, &server)),
        "the process now on the port is named: {error:?}"
    );
    assert_untouched(&second, &second_ports);
}

/// Kossé never stops itself, by identity or when a port names it.
#[test]
fn kosse_itself_is_refused() {
    let server = Server::start();
    let kosse_pid = server.child.0.id();
    let start = live_start_time(kosse_pid).expect("server start time");
    observed(&server, |service| service.process.identity.pid == kosse_pid);
    let dir = Path::new(ANYWHERE);
    let own_port = server.addr.port().to_string();

    let by_identity = kosse(
        dir,
        &server,
        &[
            "stop",
            &format!("{kosse_pid}.{start}"),
            "--ports",
            &own_port,
            "--json",
        ],
    );
    assert_eq!(
        by_identity.status.code(),
        Some(3),
        "{}",
        stderr(&by_identity)
    );
    let error = error_of(&by_identity);
    assert_eq!(error.error, CommandErrorCode::ActionRefused);
    assert_eq!(
        error.result,
        Some(ActionResult::Refused {
            reason: Ineligible::KosseItself
        })
    );

    let by_port = kosse(dir, &server, &["stop", &own_port, "--json"]);
    assert_eq!(by_port.status.code(), Some(3), "{}", stderr(&by_port));
    let error = error_of(&by_port);
    assert_eq!(error.error, CommandErrorCode::ActionRefused);
    assert!(
        error.confirm.is_empty(),
        "no command to stop Kossé: {error:?}"
    );

    assert_eq!(server.get("/api/health").status, 200, "Kossé still serves");
    assert_eq!(live_start_time(kosse_pid), Some(start));
}

/// A supervisor restarting its child is reported (exit 1), and the supervisor is never signalled.
#[test]
fn a_respawn_is_reported_as_a_negative_outcome() {
    let supervisor =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).supervise()).expect("supervisor");
    let port = supervisor.record().endpoints[0].port();
    let server = server_acting_on(&[&supervisor]);
    let child = observed(&server, |service| {
        service.process.parent_pid == Some(supervisor.pid())
            && service
                .endpoints
                .iter()
                .any(|endpoint| endpoint.port == port)
    });
    let child_id = child.process.identity;
    let start = child_id.started_at_secs.expect("start");

    let output = kosse(
        Path::new(ANYWHERE),
        &server,
        &[
            "stop",
            &format!("{}.{start}", child_id.pid),
            "--ports",
            &port.to_string(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("outcome: respawned\n"), "{text}");
    assert!(
        text.contains(&format!("parent: {} ", supervisor.pid())),
        "{text}"
    );
    assert_eq!(live_start_time(child_id.pid), None, "the child exited");
    assert_eq!(
        live_start_time(supervisor.pid()),
        Some(supervisor.record().started_at_secs),
        "the supervisor is never signalled"
    );
}

/// D1: without a verified instance nothing is signalled, and the error names how to start one.
#[test]
fn without_an_instance_nothing_is_signalled() {
    let fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    let ports = live_ports(&fixture);
    // A loopback port nothing listens on once released.
    let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("free port")
        .port();
    let id = format!("{}.{}", fixture.pid(), fixture.record().started_at_secs);
    let run = |json: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .args(["stop", &id, "--ports", &joined(&ports)])
            .args(["--instance-port", &free.to_string()])
            .args(json)
            .output()
            .expect("run kosse")
    };

    let output = run(&[]);
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    assert!(
        stderr(&output).contains(&format!("`kosse serve --no-open --port {free}`")),
        "{}",
        stderr(&output)
    );
    let output = run(&["--json"]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(error_of(&output).error, CommandErrorCode::NoInstance);
    assert_untouched(&fixture, &ports);
}
