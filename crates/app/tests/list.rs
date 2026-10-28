//! `stackscan list` through the compiled binary, checked against owned fixtures and `lsof`.
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

use stackscan_core::model::{SNAPSHOT_SCHEMA_VERSION, ScanWarning, Service, Snapshot};
use stackscan_fixtures::{Fixture, FixtureSpec, lsof};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

fn stackscan(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_stackscan"))
        .args(args)
        .output()
        .expect("run stackscan");
    assert!(
        output.status.success(),
        "stackscan {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fixture_service<'a>(snapshot: &'a Snapshot, fixture: &Fixture) -> &'a Service {
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

    let output = stackscan(&["list", "--json"]);
    let snapshot: Snapshot =
        serde_json::from_slice(&output.stdout).expect("JSON snapshot contract");
    assert_eq!(snapshot.schema_version, SNAPSHOT_SCHEMA_VERSION);
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
    let output = stackscan(&["list"]);
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
    let mut expected = vec![
        vec![
            pid.as_str(),
            "stackscan-fixture",
            shared_port.as_str(),
            "127.0.0.1,",
            "::1",
        ],
        vec![pid.as_str(), "stackscan-fixture", v6_port.as_str(), "::1"],
    ];
    // Rows are ordered by port; ephemeral ports need not ascend in spawn order.
    expected.sort_by_key(|row| row[2].parse::<u16>().expect("port"));
    assert_eq!(
        rows, expected,
        "one row per port, both addresses on the shared one"
    );
}

#[test]
fn list_rejects_unknown_flags_as_usage_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_stackscan"))
        .args(["list", "--yaml"])
        .output()
        .expect("run stackscan");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn closed_stdout_and_stderr_do_not_crash_list() {
    for args in [&["list"][..], &["list", "--json"][..]] {
        // Both read ends are closed before stackscan starts, so every write fails with EPIPE.
        let (stdout_reader, stdout_writer) = std::io::pipe().expect("stdout pipe");
        let (stderr_reader, stderr_writer) = std::io::pipe().expect("stderr pipe");
        drop((stdout_reader, stderr_reader));
        let status = Command::new(env!("CARGO_BIN_EXE_stackscan"))
            .args(args)
            .stdout(stdout_writer)
            .stderr(stderr_writer)
            .status()
            .expect("run stackscan");
        assert_eq!(status.code(), Some(0), "stackscan {args:?}: {status}");
    }
}
