//! `kosse wait` through the compiled binary, against owned fixtures: in process (no instance,
//! `--instance-port 0`) and following a running `kosse serve` that probes nothing on its own
//! (`--probe-only=`). Requested probes go only to the fixture the wait names; every request
//! counted carries the observing instance's ID, so another Kossé on the machine does not disturb
//! the counts.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    io::{BufRead, BufReader, Read},
    net::{IpAddr, Ipv4Addr, TcpListener},
    process::{Child, ChildStderr, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use kosse_core::{
    agent::{AGENT_SCHEMA_VERSION, CommandError, CommandErrorCode, WaitAnswer, WaitCondition},
    model::{HttpProbe, ProbeResult},
};
use kosse_fixtures::{Fixture, FixtureSpec, protocol::Serve};
use support::{NO_CONFIG, Server, markdown_tables, sent_by};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// Longer than any condition here takes; a wait that ends must end well before it.
const TIMEOUT: &str = "60s";
/// A scan (3 s with an instance), the probe and slack.
const EXIT_WITHIN: Duration = Duration::from_secs(20);

/// A running `kosse wait`, killed (by this handle only) if the test ends early.
struct Waiting {
    child: Child,
    stderr: BufReader<ChildStderr>,
}

impl Waiting {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .arg("wait")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn kosse wait");
        let stderr = BufReader::new(child.stderr.take().expect("piped stderr"));
        Self { child, stderr }
    }

    /// Blocks until the wait reports that its first observation did not meet the condition.
    fn until_waiting(&mut self) {
        let mut line = String::new();
        self.stderr.read_line(&mut line).expect("read stderr");
        assert!(line.starts_with("kosse: waiting up to "), "{line:?}");
    }

    /// Exit status, stdout and the rest of stderr, once it exits (bounded by [`EXIT_WITHIN`]).
    fn finish(mut self) -> (ExitStatus, String, String) {
        let deadline = Instant::now() + EXIT_WITHIN;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("poll") {
                break status;
            }
            assert!(Instant::now() < deadline, "kosse wait did not end in time");
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut stdout = String::new();
        if let Some(mut out) = self.child.stdout.take() {
            out.read_to_string(&mut stdout).expect("read stdout");
        }
        let mut stderr = String::new();
        self.stderr
            .read_to_string(&mut stderr)
            .expect("read stderr");
        (status, stdout, stderr)
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port()
}

fn port_of(fixture: &Fixture) -> u16 {
    fixture.record().endpoints[0].port()
}

fn identity_cell(fixture: &Fixture) -> String {
    format!("{}.{}", fixture.pid(), fixture.record().started_at_secs)
}

/// The `PID.START` cells of the service table on stdout.
fn identities(stdout: &str) -> Vec<String> {
    let tables = markdown_tables(stdout);
    let [table] = tables.as_slice() else {
        panic!("one table:\n{stdout}");
    };
    table
        .rows
        .iter()
        .map(|row| table.cell(row, "PID.START").to_owned())
        .collect()
}

/// `--instance-port` for both observation paths: in process, and a running instance's events.
fn paths(server: &Server) -> [(&'static str, String); 2] {
    [
        ("in process", "0".to_owned()),
        ("instance", server.addr.port().to_string()),
    ]
}

#[test]
fn wait_returns_once_a_late_fixture_listens() {
    let server = Server::start();
    for (path, instance) in paths(&server) {
        let port = free_port().to_string();
        let mut waiting = Waiting::start(&[
            "--port",
            &port,
            "--timeout",
            TIMEOUT,
            "--instance-port",
            &instance,
        ]);
        waiting.until_waiting();
        let fixture = Fixture::spawn(
            &FixtureSpec::new()
                .listen(&[V4])
                .port(port.parse().expect("port")),
        )
        .expect("fixture");
        let (status, stdout, stderr) = waiting.finish();
        assert_eq!(status.code(), Some(0), "{path}: {stderr}");
        assert_eq!(identities(&stdout), [identity_cell(&fixture)], "{path}");
    }
}

#[test]
fn wait_gone_returns_once_the_fixture_exits() {
    let server = Server::start();
    for (path, instance) in paths(&server) {
        let mut fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
        let port = port_of(&fixture).to_string();
        let mut waiting = Waiting::start(&[
            "--port",
            &port,
            "--gone",
            "--timeout",
            TIMEOUT,
            "--instance-port",
            &instance,
        ]);
        waiting.until_waiting();
        fixture.stop().expect("stop the fixture");
        let (status, stdout, stderr) = waiting.finish();
        assert_eq!(status.code(), Some(0), "{path}: {stderr}");
        assert_eq!(
            identities(&stdout),
            Vec::<String>::new(),
            "{path}: nothing listens"
        );
    }
}

/// `--http` asks for probes, through the instance's `POST /api/probe` or in process, of the
/// named endpoint only, within the one-a-second limit, and reports the URL the probe saw.
#[test]
fn wait_http_requests_probes_of_only_its_target() {
    let server = Server::start();
    let spawn = || {
        Fixture::spawn(
            &FixtureSpec::new()
                .listen(&[V4])
                .serve(Serve::Title)
                .record(),
        )
        .expect("fixture")
    };
    for (path, instance) in paths(&server) {
        let (target, bystander) = (spawn(), spawn());
        let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .args([
                "wait",
                "--port",
                &port_of(&target).to_string(),
                "--http",
                "--json",
                "--timeout",
                TIMEOUT,
                "--instance-port",
                &instance,
            ])
            .output()
            .expect("run kosse wait");
        assert!(
            output.status.success(),
            "{path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let answer: WaitAnswer = serde_json::from_slice(&output.stdout).expect("WaitAnswer");
        assert_eq!(answer.schema_version, AGENT_SCHEMA_VERSION);
        assert_eq!((answer.condition, answer.met), (WaitCondition::Http, true));
        let [service] = answer.services.as_slice() else {
            panic!("{path}: one service: {answer:?}");
        };
        assert_eq!(service.process.identity.pid, target.pid());
        let HttpProbe::Probed {
            requested: true,
            result: ProbeResult::Response { url, .. },
            ..
        } = &service.endpoints[0].http
        else {
            panic!("{path}: a requested probe's response: {answer:?}");
        };
        assert_eq!(answer.url.as_ref(), Some(url));

        let sent = sent_by(target.received(port_of(&target)), &answer.instance).len();
        let allowed = usize::try_from(answer.waited_ms / 1000).expect("small") + 1;
        assert!(
            (1..=allowed).contains(&sent),
            "{path}: {sent} probes in {} ms",
            answer.waited_ms
        );
        assert_eq!(
            sent_by(bystander.received(port_of(&bystander)), &answer.instance),
            Vec::<String>::new(),
            "{path}: only the named endpoint is probed"
        );
        if instance != "0" {
            assert_eq!(
                answer.instance,
                server.snapshot().instance,
                "observed through the running instance"
            );
        }
    }
}

/// A listener that answers, but not over HTTP, ends the wait at once with its probe state rather
/// than waiting out the timeout.
#[test]
fn wait_http_on_a_tcp_only_fixture_ends_with_its_probe_state() {
    let fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    let port = port_of(&fixture).to_string();
    let waiting = Waiting::start(&[
        "--port",
        &port,
        "--http",
        "--timeout",
        TIMEOUT,
        "--instance-port",
        "0",
    ]);
    let (status, stdout, stderr) = waiting.finish();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(&format!(":{port} not HTTP")), "{stderr}");
    let tables = markdown_tables(&stdout);
    let row = &tables[0].rows[0];
    assert_eq!(tables[0].cell(row, "HTTP"), "not HTTP");

    let json = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", NO_CONFIG)
        .args([
            "wait",
            "--port",
            &port,
            "--http",
            "--json",
            "--instance-port",
            "0",
        ])
        .output()
        .expect("run kosse wait");
    assert_eq!(json.status.code(), Some(1));
    let error: CommandError = serde_json::from_slice(&json.stderr).expect("CommandError");
    assert_eq!(error.error, CommandErrorCode::NoHttp);
    let answer: WaitAnswer = serde_json::from_slice(&json.stdout).expect("WaitAnswer");
    assert!(!answer.met);
}

/// A timeout is a negative answer (1) that still prints the last observation; malformed
/// arguments are usage errors (2).
#[test]
fn wait_times_out_with_the_last_observation_and_rejects_bad_arguments() {
    let port = free_port().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", NO_CONFIG)
        .args([
            "wait",
            "--port",
            &port,
            "--timeout",
            "1s",
            "--json",
            "--instance-port",
            "0",
        ])
        .output()
        .expect("run kosse wait");
    assert_eq!(output.status.code(), Some(1));
    let error: CommandError = serde_json::from_slice(&output.stderr).expect("CommandError");
    assert_eq!(error.error, CommandErrorCode::TimedOut);
    let answer: WaitAnswer = serde_json::from_slice(&output.stdout).expect("WaitAnswer");
    assert_eq!((answer.met, answer.services.len()), (false, 0));
    assert!(answer.waited_ms >= 1000, "{}", answer.waited_ms);

    for args in [
        &["wait"][..],
        &["wait", "web", "--port", "1"][..],
        &["wait", "--port", "1", "--http", "--gone"][..],
        &["wait", "--port", "1", "--timeout", "5x"][..],
    ] {
        let status = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .args(args)
            .output()
            .expect("run kosse")
            .status;
        assert_eq!(status.code(), Some(2), "{args:?}");
    }
}

/// `--gone` never reports success for something it never saw: a service name no profile in scope
/// configures is `notFound` at once, under every condition.
#[test]
fn wait_rejects_a_service_name_configured_nowhere() {
    for condition in [&[][..], &["--gone"][..], &["--http"][..]] {
        let mut args = vec![
            "wait",
            "no-such-service",
            "--json",
            "--timeout",
            TIMEOUT,
            "--instance-port",
            "0",
        ];
        args.extend_from_slice(condition);
        let started = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .args(&args)
            .output()
            .expect("run kosse wait");
        assert!(started.elapsed() < EXIT_WITHIN, "answered at once");
        assert_eq!(output.status.code(), Some(1), "{condition:?}");
        let error: CommandError = serde_json::from_slice(&output.stderr).expect("CommandError");
        assert_eq!(error.error, CommandErrorCode::NotFound, "{condition:?}");
        assert_eq!(output.stdout, b"", "nothing was observed for it");
    }
}

/// Under `--project` with `--port`, a process still listening on the port whose project is
/// unknown (here: its working directory was deleted) may be in that project, so the port is not
/// "gone".
#[test]
fn wait_gone_is_not_met_while_an_unknown_project_process_holds_the_port() {
    let tree = kosse_fixtures::repo::Sandbox::new().expect("sandbox");
    let dir = tree.dir("app").expect("dir");
    tree.file("app/package.json", "{}").expect("manifest");
    let fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("fixture");
    std::fs::remove_dir_all(&fixture.record().cwd).expect("remove the fixture's own directory");
    let port = port_of(&fixture).to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", NO_CONFIG)
        .args([
            "wait",
            "--port",
            &port,
            "--gone",
            "--project",
            dir.to_str().expect("UTF-8"),
            "--timeout",
            "2s",
            "--instance-port",
            "0",
        ])
        .output()
        .expect("run kosse wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("was not gone") && stderr.contains("project is unknown"),
        "{stderr}"
    );
}
