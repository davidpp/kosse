//! `kosse list` through the compiled binary, checked against owned fixtures and `lsof`.
//! Only fixture rows are asserted on (or printed); the machine's other listeners are ignored.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

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
use support::{MarkdownTable, markdown_tables};

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

    let output = kosse(&["list", "--json", "--instance-port", "0"]);
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

/// The documented columns of `kosse list`'s service table (README, CONTRIBUTING).
const COLUMNS: [&str; 8] = [
    "PROJECT",
    "BRANCH",
    "SERVICE",
    "PORT",
    "HTTP",
    "PROCESS",
    "PID.START",
    "AGE",
];

/// The service table of `kosse list` output (the first table), with the documented columns.
fn service_table(stdout: &[u8]) -> MarkdownTable {
    let text = String::from_utf8(stdout.to_vec()).expect("UTF-8 table");
    let table = markdown_tables(&text)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no table in\n{text}"));
    assert_eq!(table.headers, COLUMNS, "{text}");
    table
}

fn identity_cell(fixture: &Fixture) -> String {
    format!("{}.{}", fixture.pid(), fixture.record().started_at_secs)
}

#[test]
fn list_table_shows_each_fixture_port_once() {
    let fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4, V6]).listen(&[V6])).expect("spawn fixture");
    let output = kosse(&["list", "--instance-port", "0"]);
    let table = service_table(&output.stdout);
    let rows = table.rows_where("PID.START", &identity_cell(&fixture));
    let ports: Vec<u16> = rows
        .iter()
        .map(|row| table.cell(row, "PORT").parse().expect("port"))
        .collect();
    let mut expected: Vec<u16> = lsof::tcp_listeners(&[fixture.pid()])
        .expect("lsof")
        .into_iter()
        .map(|(_, _, port)| port)
        .collect();
    expected.sort_unstable();
    expected.dedup();
    assert_eq!(ports, expected, "one row per port, not per socket (lsof)");
    for row in rows {
        // The fixture's own temporary cwd is in no project (nor configured service), and a
        // one-shot scan does not probe.
        assert_eq!(table.cell(row, "PROJECT"), "-");
        assert_eq!(table.cell(row, "BRANCH"), "-");
        assert_eq!(table.cell(row, "SERVICE"), "-");
        assert_eq!(table.cell(row, "PROCESS"), "kosse-fixture");
        assert_ne!(table.cell(row, "AGE"), "-");
    }
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

    let output = kosse(&["list", "--json", "--instance-port", "0"]);
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
    let table = service_table(&kosse(&["list", "--instance-port", "0"]).stdout);
    let rows = table.rows_where("PID.START", &identity_cell(&in_web));
    let [row] = rows.as_slice() else {
        panic!("one row for the web fixture: {rows:?}");
    };
    assert_eq!(
        [table.cell(row, "PROJECT"), table.cell(row, "BRANCH")],
        ["app/packages/web", "main"]
    );
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
        &["list", "--instance-port", "0"][..],
        &["list", "--json", "--instance-port", "0"][..],
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

fn kosse_at(dir: &std::path::Path, config: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", config)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run kosse")
}

fn pids_of(output: &Output) -> BTreeSet<u32> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout).expect("JSON snapshot");
    let observation = snapshot.observation.expect("an observation");
    for service in &observation.services {
        let root = service.project.as_deref().expect("a project");
        assert!(
            observation
                .projects
                .iter()
                .any(|project| project.root == root),
            "the filtered snapshot still lists the project of every service"
        );
    }
    observation
        .services
        .iter()
        .map(|service| service.process.identity.pid)
        .collect()
}

/// An agent asks "what runs for this worktree?": `--here` resolves the caller's directory as
/// attribution does, so two worktrees of one repository stay separate, a package directory
/// belongs to its worktree, and a directory outside any project is refused, never "everything".
#[test]
fn list_here_lists_only_the_callers_worktree() {
    let tree = Sandbox::new().expect("sandbox");
    let main = tree.dir("shop").expect("dir");
    init_repo(&main).expect("repo");
    tree.file("shop/apps/web/package.json", "{}").expect("file");
    let web = tree.dir("shop/apps/web").expect("dir");
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
    let outside = tree.dir("scratch").expect("dir");
    let spawn = |dir: &std::path::Path| {
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).cwd(dir)).expect("spawn fixture")
    };
    let (in_web, in_linked) = (spawn(&web), spawn(&linked));
    let config = "/var/empty/kosse-tests";
    let here = ["list", "--here", "--json", "--instance-port", "0"];

    assert_eq!(
        pids_of(&kosse_at(&linked, config, &here)),
        BTreeSet::from([in_linked.pid()]),
        "the linked worktree alone"
    );
    assert_eq!(
        pids_of(&kosse_at(&web, config, &here)),
        BTreeSet::from([in_web.pid()]),
        "from a package directory: its worktree, not the linked one"
    );
    let by_root = [
        "list",
        "--project",
        &path_text(&main),
        "--json",
        "--instance-port",
        "0",
    ];
    assert_eq!(
        pids_of(&kosse_at(&outside, config, &by_root)),
        BTreeSet::from([in_web.pid()]),
        "--project names the same project from anywhere"
    );

    // The table answers the same question.
    let table = service_table(
        &kosse_at(&linked, config, &["list", "--here", "--instance-port", "0"]).stdout,
    );
    let identities: Vec<&str> = table
        .rows
        .iter()
        .map(|row| table.cell(row, "PID.START"))
        .collect();
    assert_eq!(identities, [identity_cell(&in_linked)]);
    assert_eq!(table.cell(&table.rows[0], "BRANCH"), "feature");

    // Outside a project: a usage error (2), with a typed error under --json and nothing on
    // stdout.
    let refused = kosse_at(&outside, config, &here);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(refused.stdout, b"", "stdout carries only answers");
    let error: kosse_core::agent::CommandError =
        serde_json::from_slice(&refused.stderr).expect("CommandError JSON on stderr");
    assert_eq!(
        error.error,
        kosse_core::agent::CommandErrorCode::NotInProject
    );
    let plain = kosse_at(&outside, config, &here[..2]);
    assert_eq!(plain.status.code(), Some(2));
    let stderr = String::from_utf8(plain.stderr).expect("UTF-8");
    assert_eq!(stderr.lines().count(), 1, "one plain line: {stderr}");
    assert!(stderr.starts_with("kosse: "), "{stderr}");
}

/// `--port` and `--service` narrow `--json` to a snapshot of only the matching services, and the
/// port's owner is the one `lsof` sees.
#[test]
fn list_filters_by_port_and_configured_service() {
    let tree = Sandbox::new().expect("sandbox");
    let repo = tree.dir("shop").expect("dir");
    init_repo(&repo).expect("repo");
    let config = tree.root().join("config");
    std::fs::create_dir_all(&config).expect("config dir");
    let (one, two) = (
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).cwd(&repo)).expect("fixture"),
        Fixture::spawn(&FixtureSpec::new().listen(&[V4]).cwd(&repo)).expect("fixture"),
    );
    let port_of = |fixture: &Fixture| fixture.record().endpoints[0].port();
    let profile = serde_json::json!({
        "version": 1,
        "projects": { path_text(&repo): { "version": 1, "services": {
            "web": { "name": "Storefront", "match": { "port": port_of(&two) } }
        } } }
    });
    std::fs::write(config.join("config.json"), profile.to_string()).expect("write config");
    let config = path_text(&config);
    let run = |args: &[&str]| {
        let mut all = vec!["list", "--json", "--instance-port", "0"];
        all.extend_from_slice(args);
        kosse_at(tree.root(), &config, &all)
    };

    let port = port_of(&one).to_string();
    assert_eq!(
        pids_of(&run(&["--port", &port])),
        BTreeSet::from([one.pid()])
    );
    let lsof_owners: BTreeSet<u32> = lsof::tcp_listeners(&[one.pid(), two.pid()])
        .expect("lsof")
        .into_iter()
        .filter(|&(_, _, listening)| listening == port_of(&one))
        .map(|(pid, _, _)| pid)
        .collect();
    assert_eq!(lsof_owners, BTreeSet::from([one.pid()]));

    // Scoped to this project: other suites' repositories may configure a `web` too.
    let project = path_text(&repo);
    for name in ["web", "Storefront"] {
        assert_eq!(
            pids_of(&run(&["--service", name, "--project", &project])),
            BTreeSet::from([two.pid()]),
            "by configured ID or name: {name}"
        );
    }
    assert_eq!(
        pids_of(&run(&["--service", "api", "--project", &project])),
        BTreeSet::new()
    );
}

/// Process-, Git- and profile-derived text lands in table cells, so a name holding `|`,
/// backticks or a newline must not add a column or row, open a code span or end the table.
#[test]
fn hostile_names_cannot_break_the_markdown_table() {
    let tree = Sandbox::new().expect("sandbox");
    // Git's own output is line-based, so a newline in a worktree path makes the project unknown
    // (attribution's honest answer), and profiles refuse control characters in names: the
    // newline comes from the process name.
    let repo = tree.dir("shop | `x` | row |").expect("dir");
    init_repo(&repo).expect("repo");
    git(&repo, &["checkout", "-q", "-b", "fix|`rm`"]).expect("branch");
    let config = tree.root().join("config");
    std::fs::create_dir_all(&config).expect("config dir");
    let fixture = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .cwd(&repo)
            .executable_name("srv|`x`\n|y")
            .args(&["a | b", "`rm -rf`", "line\n| injected |"]),
    )
    .expect("fixture");
    let profile = serde_json::json!({
        "version": 1,
        "projects": { path_text(&repo): { "version": 1, "services": {
            "web": { "name": "Web | `x`", "match": { "port": fixture.record().endpoints[0].port() } },
            "api": { "name": "API | `y`" }
        } } }
    });
    std::fs::write(config.join("config.json"), profile.to_string()).expect("write config");

    let output = kosse_at(
        tree.root(),
        &path_text(&config),
        &[
            "list",
            "--project",
            &path_text(&repo),
            "--instance-port",
            "0",
        ],
    );
    // Profile warnings are machine-wide; only one about this project's profile would matter.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success()
            && !stderr
                .lines()
                .any(|line| line.contains("profile") && line.contains(&path_text(&repo))),
        "the profile applies: {stderr}"
    );
    // `markdown_tables` fails on a broken row, a stray code span or a table cut short.
    let text = String::from_utf8(output.stdout.clone()).expect("UTF-8");
    let tables = markdown_tables(&text);
    assert_eq!(
        tables.len(),
        2,
        "services and configured-not-observed:\n{text}"
    );
    let table = service_table(&output.stdout);
    let [row] = table.rows.as_slice() else {
        panic!("one row for the one port:\n{text}");
    };
    // Control characters are escaped (`\u{a}`), never passed through.
    let escaped = |raw: &str| raw.replace('\n', "\\u{a}");
    let project = repo
        .file_name()
        .and_then(|name| name.to_str())
        .expect("name");
    assert_eq!(table.cell(row, "PROJECT"), escaped(project));
    assert_eq!(table.cell(row, "BRANCH"), "fix|`rm`");
    assert_eq!(table.cell(row, "SERVICE"), "Web | `x` (web) [central]");
    assert_eq!(table.cell(row, "PROCESS"), escaped("srv|`x`\n|y"));
    assert_eq!(table.cell(row, "PID.START"), identity_cell(&fixture));
    let unobserved = &tables[1];
    assert_eq!(
        unobserved.cell(&unobserved.rows[0], "SERVICE"),
        "API | `y` (api) [central]"
    );
}

/// Exit codes distinguish a usage error (2, like a directory outside any project above) from not
/// being able to observe at all (4): here an unusable configuration directory.
#[test]
fn list_exit_codes_tell_usage_from_unobservable() {
    let conflicting = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "/var/empty/kosse-tests")
        .args(["list", "--here", "--project", "/", "--instance-port", "0"])
        .output()
        .expect("run kosse");
    assert_eq!(conflicting.status.code(), Some(2));
    let relative = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "relative/config")
        .args(["list", "--json", "--instance-port", "0"])
        .output()
        .expect("run kosse");
    assert_eq!(relative.status.code(), Some(4));
    assert_eq!(relative.stdout, b"", "stdout carries only answers");
    let error: kosse_core::agent::CommandError =
        serde_json::from_slice(&relative.stderr).expect("CommandError JSON on stderr");
    assert_eq!(
        error.error,
        kosse_core::agent::CommandErrorCode::ConfigUnavailable
    );
}
