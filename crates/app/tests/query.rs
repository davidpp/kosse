//! `kosse url` and `kosse port` through the compiled binary, against owned fixtures in temporary
//! repositories, a running `kosse serve` that probes only those fixtures (`--probe-only`), and
//! `lsof`. Only fixture rows are asserted on.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, TcpListener},
    path::Path,
    process::{Command, Output},
    time::Duration,
};

use kosse_core::{
    agent::{
        AGENT_SCHEMA_VERSION, CommandError, CommandErrorCode, PortAnswer, PortState, UrlAnswer,
    },
    model::{HttpProbe, ProbeResult, ProcessIdentity, ScanWarning, Snapshot},
};
use kosse_fixtures::{
    Fixture, FixtureSpec, lsof,
    protocol::Serve,
    repo::{Sandbox, git, init_repo},
};
use support::{NO_CONFIG, Server, eventually, markdown_tables};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// First scan, the probes and the next scan.
const PROBED_WITHIN: Duration = Duration::from_secs(12);

fn kosse(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", NO_CONFIG)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run kosse")
}

fn path_text(path: &Path) -> String {
    path.to_str().expect("UTF-8 path").to_owned()
}

fn port_of(fixture: &Fixture) -> u16 {
    fixture.record().endpoints[0].port()
}

fn identity(fixture: &Fixture) -> ProcessIdentity {
    ProcessIdentity {
        pid: fixture.pid(),
        started_at_secs: Some(fixture.record().started_at_secs),
    }
}

fn error_of(output: &Output) -> CommandError {
    assert!(output.stdout.is_empty(), "stdout carries only answers");
    serde_json::from_slice(&output.stderr).unwrap_or_else(|_| {
        panic!(
            "not a CommandError: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// The URL the instance's snapshot holds for the fixture's port, once it answered HTTP.
fn probed_url(snapshot: &Snapshot, fixture: &Fixture) -> Option<String> {
    let service = snapshot
        .observation
        .as_ref()?
        .services
        .iter()
        .find(|service| service.process.identity == identity(fixture))?;
    match &service.endpoints.first()?.http {
        HttpProbe::Probed {
            result: ProbeResult::Response { url, .. },
            ..
        } => Some(url.clone()),
        _ => None,
    }
}

const REPO_PROFILE: &str =
    r#"{ "version": 1, "services": { "web": { "match": { "cwd": "." } } } }"#;

/// The agent's "which URL?" through a running instance: a service name shared by two worktrees
/// is ambiguous until scoped with `--here`; then (or by port) `url` prints exactly the URL the
/// instance's probe saw, and `port` names the same owner `lsof` does.
#[test]
fn url_and_port_answer_from_a_running_instance() {
    let tree = Sandbox::new().expect("sandbox");
    let main = tree.dir("shop").expect("dir");
    init_repo(&main).expect("repo");
    tree.file("shop/kosse.json", REPO_PROFILE).expect("file");
    git(&main, &["add", "-A"]).expect("add");
    git(&main, &["commit", "-q", "-m", "profile"]).expect("commit");
    let linked = tree.root().join("shop-feature");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            &path_text(&linked),
        ],
    )
    .expect("worktree add");
    let linked = linked.canonicalize().expect("canonical");
    let spawn = |dir: &Path| {
        Fixture::spawn(
            &FixtureSpec::new()
                .listen(&[V4])
                .serve(Serve::Title)
                .cwd(dir),
        )
        .expect("fixture")
    };
    let (in_main, in_linked) = (spawn(&main), spawn(&linked));
    let server = Server::start_with(&[&format!(
        "--probe-only={},{}",
        in_main.pid(),
        in_linked.pid()
    )]);
    let instance = server.addr.port().to_string();
    let instance = instance.as_str();
    let expected = eventually(PROBED_WITHIN, || {
        let snapshot = server.snapshot();
        probed_url(&snapshot, &in_main)?;
        probed_url(&snapshot, &in_linked)
    })
    .expect("both fixtures answered the instance's probes");

    let ambiguous = kosse(
        tree.root(),
        &["url", "web", "--json", "--instance-port", instance],
    );
    assert_eq!(ambiguous.status.code(), Some(1));
    assert_eq!(error_of(&ambiguous).error, CommandErrorCode::Ambiguous);

    let here = kosse(
        &linked,
        &["url", "web", "--here", "--instance-port", instance],
    );
    assert_eq!(
        (here.status.code(), String::from_utf8_lossy(&here.stdout)),
        (Some(0), format!("{expected}\n").into()),
        "{}",
        String::from_utf8_lossy(&here.stderr)
    );

    let port = port_of(&in_linked).to_string();
    let by_port = kosse(
        tree.root(),
        &["url", &port, "--json", "--instance-port", instance],
    );
    assert!(by_port.status.success());
    let answer: UrlAnswer = serde_json::from_slice(&by_port.stdout).expect("UrlAnswer");
    assert_eq!(
        answer,
        UrlAnswer {
            schema_version: AGENT_SCHEMA_VERSION,
            url: expected,
            port: port_of(&in_linked),
            identity: identity(&in_linked),
            project: Some(path_text(&linked)),
            service: Some("web".to_owned()),
        }
    );

    port_names_the_owner(tree.root(), &in_linked, &in_main, &linked, instance);
    let (status, _) = server.terminate();
    assert!(status.success());
}

/// `kosse port` for the port of `owner` (in project `root`, next to `other`): its record names
/// the owner `lsof` sees, and the table has exactly its row.
fn port_names_the_owner(dir: &Path, owner: &Fixture, other: &Fixture, root: &Path, instance: &str) {
    let port = port_of(owner).to_string();
    let output = kosse(dir, &["port", &port, "--json", "--instance-port", instance]);
    assert!(output.status.success());
    let answer: PortAnswer = serde_json::from_slice(&output.stdout).expect("PortAnswer");
    assert_eq!(answer.schema_version, AGENT_SCHEMA_VERSION);
    assert_eq!(answer.state, PortState::Taken);
    let pids: BTreeSet<u32> = answer
        .services
        .iter()
        .map(|service| service.process.identity.pid)
        .collect();
    let lsof_pids: BTreeSet<u32> = lsof::tcp_listeners(&[other.pid(), owner.pid()])
        .expect("lsof")
        .into_iter()
        .filter(|&(_, _, listening)| listening == port_of(owner))
        .map(|(pid, _, _)| pid)
        .collect();
    assert_eq!(pids, lsof_pids, "port --json vs lsof");
    let roots: Vec<&str> = answer
        .projects
        .iter()
        .map(|project| project.root.as_str())
        .collect();
    assert_eq!(roots, [path_text(root)]);

    let table = kosse(dir, &["port", &port, "--instance-port", instance]);
    assert!(table.status.success());
    let text = String::from_utf8(table.stdout).expect("UTF-8");
    let tables = markdown_tables(&text);
    let [services] = tables.as_slice() else {
        panic!("one table:\n{text}");
    };
    let [row] = services.rows.as_slice() else {
        panic!("one row:\n{text}");
    };
    assert_eq!(
        services.cell(row, "PID.START"),
        format!("{}.{}", owner.pid(), owner.record().started_at_secs)
    );
    assert_eq!(services.cell(row, "BRANCH"), "feature");
    assert_eq!(services.cell(row, "SERVICE"), "web [repository]");
    assert!(
        services
            .cell(row, "HTTP")
            .starts_with("200 http://localhost:"),
        "{text}"
    );
}

/// Without an HTTP answer there is no URL: exit 1 with the endpoint's probe state, as one plain
/// line or a typed error. A one-shot scan never probes, so it says so.
#[test]
fn url_without_an_http_answer_is_a_negative_answer() {
    let tree = Sandbox::new().expect("sandbox");
    let dir = tree.dir("app").expect("dir");
    tree.file("app/package.json", "{}").expect("manifest");
    let fixture = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .serve(Serve::Title)
            .cwd(&dir),
    )
    .expect("fixture");
    let port = port_of(&fixture).to_string();

    let plain = kosse(tree.root(), &["url", &port, "--instance-port", "0"]);
    assert_eq!(plain.status.code(), Some(1));
    assert_eq!(plain.stdout, b"", "stdout carries only answers");
    let stderr = String::from_utf8(plain.stderr).expect("UTF-8");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.contains(&format!(":{port} pending")) && stderr.contains("never probes"),
        "{stderr}"
    );
    let json = kosse(
        tree.root(),
        &["url", &port, "--json", "--instance-port", "0"],
    );
    assert_eq!(json.status.code(), Some(1));
    assert_eq!(error_of(&json).error, CommandErrorCode::NoHttp);

    let unknown = kosse(
        tree.root(),
        &["url", "no-such-service", "--json", "--instance-port", "0"],
    );
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(error_of(&unknown).error, CommandErrorCode::NotFound);
    for usage in [&["url"][..], &["port", "http"][..]] {
        assert_eq!(
            kosse(tree.root(), usage).status.code(),
            Some(2),
            "{usage:?}"
        );
    }
}

/// A port nobody this user can see listens on is `free`, qualified: other users' sockets are
/// invisible without administrator rights, so it is never a definite free.
#[test]
fn port_is_free_only_as_far_as_this_user_can_see() {
    let port = {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        listener.local_addr().expect("address").port()
    };
    let here = std::env::temp_dir();
    let json = kosse(
        &here,
        &["port", &port.to_string(), "--json", "--instance-port", "0"],
    );
    assert!(json.status.success());
    let answer: PortAnswer = serde_json::from_slice(&json.stdout).expect("PortAnswer");
    assert_eq!((answer.state, answer.services.len()), (PortState::Free, 0));

    // Whether this user sees other users' sockets is what the scan itself reports.
    let listed = kosse(&here, &["list", "--json", "--instance-port", "0"]);
    let snapshot: Snapshot = serde_json::from_slice(&listed.stdout).expect("snapshot");
    let hidden = snapshot
        .observation
        .expect("observation")
        .warnings
        .contains(&ScanWarning::OtherUsersNotVisible);
    assert_eq!(answer.other_users_visible, !hidden);

    let plain = kosse(&here, &["port", &port.to_string(), "--instance-port", "0"]);
    assert!(plain.status.success());
    let line = String::from_utf8(plain.stdout).expect("UTF-8");
    assert!(line.starts_with("free"), "{line}");
    assert_eq!(
        line.contains("as far as this user can see"),
        hidden,
        "{line}"
    );
}
