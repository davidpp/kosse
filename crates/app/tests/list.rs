//! `kosse list` through the compiled binary, checked against owned fixtures and `lsof`.
//! Only fixture rows are asserted on (or printed); the machine's other listeners are ignored.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    process::{Command, Output},
};

use kosse_core::model::{
    CpuUsage, GitHead, Observation, Project, SNAPSHOT_SCHEMA_VERSION, ScanWarning, Service,
    Snapshot,
};
use kosse_fixtures::{
    Fixture, FixtureSpec, lsof,
    repo::{Sandbox, git, init_repo},
};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

fn kosse(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .args(args)
        .output()
        .expect("run kosse");
    assert!(
        output.status.success(),
        "kosse {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fixture_service<'a>(snapshot: &'a Observation, fixture: &Fixture) -> &'a Service {
    let mut matching = snapshot
        .services
        .iter()
        .filter(|service| service.process.identity.pid == fixture.pid());
    let service = matching
        .next()
        .unwrap_or_else(|| panic!("fixture {} missing from list --json", fixture.pid()));
    assert!(matching.next().is_none(), "fixture listed twice");
    service
}

fn binds(service: &Service) -> BTreeSet<(u32, IpAddr, u16)> {
    service
        .endpoints
        .iter()
        .flat_map(|endpoint| {
            endpoint.bind_addresses.iter().map(|address| {
                (
                    service.process.identity.pid,
                    address.parse().expect("IP address"),
                    endpoint.port,
                )
            })
        })
        .collect()
}

#[test]
fn list_json_matches_fixture_manifest_and_lsof() {
    let fixtures = [
        Fixture::spawn(&FixtureSpec::new().listen(&[V4])),
        Fixture::spawn(&FixtureSpec::new().listen(&[V6])),
        Fixture::spawn(&FixtureSpec::new().listen(&[V4, V6]).listen(&[V4])),
    ]
    .map(|fixture| fixture.expect("spawn fixture"));

    let output = kosse(&["list", "--json", "--port", "0"]);
    let published: Snapshot =
        serde_json::from_slice(&output.stdout).expect("JSON snapshot contract");
    assert_eq!(published.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(published.revision, 1);
    assert_ne!(published.instance, "");
    assert_eq!(published.scanner.interval_ms, None, "one-shot");
    assert_eq!(published.scanner.failure, None);
    let snapshot = published.observation.expect("an observation");
    assert!(snapshot.scan_duration_ms > 0.0);
    assert!(
        output.stderr.is_empty(),
        "JSON mode keeps warnings in the document"
    );

    for fixture in &fixtures {
        let service = fixture_service(&snapshot, fixture);
        let manifest: BTreeSet<_> = fixture
            .record()
            .endpoints
            .iter()
            .map(|endpoint| (fixture.pid(), endpoint.ip(), endpoint.port()))
            .collect();
        assert_eq!(binds(service), manifest, "list --json vs manifest");
        assert_eq!(
            binds(service),
            lsof::tcp_listeners(&[fixture.pid()]).expect("lsof"),
            "list --json vs lsof"
        );
        assert_eq!(
            service.process.identity.started_at_secs,
            Some(fixture.record().started_at_secs)
        );
        assert_eq!(
            service.process.cwd.as_deref(),
            fixture.record().cwd.to_str()
        );
        // One observation: CPU cannot be sampled yet, memory can.
        assert_eq!(service.process.metrics.cpu, CpuUsage::WarmingUp);
        assert!(
            service
                .process
                .metrics
                .memory_bytes
                .is_some_and(|bytes| bytes > 0)
        );
        assert!(service.process.age_secs.is_some());
    }

    let dual = fixture_service(&snapshot, &fixtures[2]);
    assert_eq!(dual.endpoints.len(), 2, "two ports, not three sockets");
    assert_eq!(
        dual.endpoints[0].bind_addresses.len() + dual.endpoints[1].bind_addresses.len(),
        3
    );

    let is_root = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .is_ok_and(|output| output.stdout.trim_ascii() == b"0");
    assert_eq!(
        snapshot
            .warnings
            .contains(&ScanWarning::OtherUsersNotVisible),
        !is_root
    );
}

#[test]
fn list_table_shows_each_fixture_port_once_with_its_addresses() {
    let fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4, V6]).listen(&[V6])).expect("spawn fixture");
    let output = kosse(&["list", "--port", "0"]);
    let table = String::from_utf8(output.stdout).expect("UTF-8 table");
    let pid = fixture.pid().to_string();

    let rows: Vec<Vec<&str>> = table
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|columns| columns.first() == Some(&pid.as_str()))
        .collect();
    let endpoints = &fixture.record().endpoints;
    let shared_port = endpoints[0].port().to_string();
    let v6_port = endpoints[2].port().to_string();
    // The fixture's own temporary cwd is in no project (nor configured service).
    let mut expected = vec![
        vec![
            pid.as_str(),
            "kosse-fixture",
            shared_port.as_str(),
            "-",
            "-",
            "-",
            "127.0.0.1,",
            "::1",
        ],
        vec![
            pid.as_str(),
            "kosse-fixture",
            v6_port.as_str(),
            "-",
            "-",
            "-",
            "::1",
        ],
    ];
    // Rows are ordered by port; ephemeral ports need not ascend in spawn order.
    expected.sort_by_key(|row| row[2].parse::<u16>().expect("port"));
    assert_eq!(
        rows, expected,
        "one row per port, both addresses on the shared one"
    );
}

fn project_of<'a>(observation: &'a Observation, fixture: &Fixture) -> &'a Project {
    let root = fixture_service(observation, fixture)
        .project
        .as_deref()
        .unwrap_or_else(|| panic!("fixture {} has no project", fixture.pid()));
    observation
        .projects
        .iter()
        .find(|project| project.root == root)
        .expect("project listed")
}

fn path_text(path: &std::path::Path) -> String {
    path.to_str().expect("UTF-8 path").to_owned()
}

/// The user-facing acceptance path: `kosse list --json` distinguishes same-name
/// repositories, worktrees, packages and detached HEADs of real processes.
#[test]
fn list_json_attributes_projects_worktrees_and_packages() {
    let tree = Sandbox::new().expect("sandbox");
    let first = tree.dir("one/app").expect("dir");
    let second = tree.dir("two/app").expect("dir");
    for repo in [&first, &second] {
        init_repo(repo).expect("repo");
    }
    tree.file("one/app/package.json", "{}").expect("file");
    tree.file("one/app/packages/web/package.json", "{}")
        .expect("file");
    let web = tree.dir("one/app/packages/web").expect("dir");
    let linked = tree.root().join("one/app wt é");
    git(
        &first,
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
    git(&second, &["checkout", "-q", "--detach"]).expect("detach");
    let spawn = |dir: &std::path::Path| {
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).cwd(dir)).expect("spawn fixture")
    };
    let (in_web, in_linked, in_second) = (spawn(&web), spawn(&linked), spawn(&second));

    let output = kosse(&["list", "--json", "--port", "0"]);
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout).expect("JSON");
    let observation = snapshot.observation.expect("an observation");

    let web_project = project_of(&observation, &in_web);
    let linked_project = project_of(&observation, &in_linked);
    let second_project = project_of(&observation, &in_second);
    assert_eq!(web_project.root, path_text(&first));
    assert_eq!(linked_project.root, path_text(&linked));
    assert_eq!(second_project.root, path_text(&second));
    assert_eq!(web_project.name, second_project.name, "same name");
    let (web_git, linked_git, second_git) = (
        web_project.git.as_ref().expect("git"),
        linked_project.git.as_ref().expect("git"),
        second_project.git.as_ref().expect("git"),
    );
    assert_eq!(web_git.common_dir, linked_git.common_dir);
    assert_ne!(web_git.common_dir, second_git.common_dir);
    assert!(linked_git.linked);
    assert_eq!(
        linked_git.head,
        GitHead::Branch {
            name: "feature".into()
        }
    );
    assert!(matches!(second_git.head, GitHead::Detached { .. }));
    let package = fixture_service(&observation, &in_web)
        .package
        .as_ref()
        .expect("package");
    assert_eq!(package.path, "packages/web");

    // The table shows the same attribution.
    let table = String::from_utf8(kosse(&["list", "--port", "0"]).stdout).expect("UTF-8");
    let row = table
        .lines()
        .find(|line| line.split_whitespace().next() == Some(&in_web.pid().to_string()))
        .expect("web fixture row");
    let columns: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(columns[3..5], ["app/packages/web", "main"], "{row}");
}

#[test]
fn list_rejects_unknown_flags_as_usage_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .args(["list", "--yaml"])
        .output()
        .expect("run kosse");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn closed_stdout_and_stderr_do_not_crash_list() {
    for args in [
        &["list", "--port", "0"][..],
        &["list", "--json", "--port", "0"][..],
    ] {
        // Both read ends are closed before kosse starts, so every write fails with EPIPE.
        let (stdout_reader, stdout_writer) = std::io::pipe().expect("stdout pipe");
        let (stderr_reader, stderr_writer) = std::io::pipe().expect("stderr pipe");
        drop((stdout_reader, stderr_reader));
        let status = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
            .args(args)
            .stdout(stdout_writer)
            .stderr(stderr_writer)
            .status()
            .expect("run kosse");
        assert_eq!(status.code(), Some(0), "kosse {args:?}: {status}");
    }
}
