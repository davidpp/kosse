//! Discovery against real owned fixture processes, checked against their manifest and against
//! `lsof`. Other listeners on the machine are ignored (and never printed).
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime},
};

use stackscan_core::{
    inspect::{ProcessInspector, ProcessState},
    model::{CpuUsage, Exposure, Observation, ProcessField, ScanWarning, Service},
    scan::Scanner,
    sockets::{ListenSocket, tcp_listen_sockets},
};
use stackscan_fixtures::{Fixture, FixtureSpec, lsof};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

fn spawn(spec: &FixtureSpec) -> Fixture {
    Fixture::spawn(spec).expect("spawn fixture")
}

fn service_of<'a>(snapshot: &'a Observation, fixture: &Fixture) -> &'a Service {
    let matching: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.process.identity.pid == fixture.pid())
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "fixture {} must be exactly one service",
        fixture.pid()
    );
    matching[0]
}

/// `(pid, address, port)` for every bind address the snapshot reports for this fixture.
fn observed_binds(service: &Service) -> BTreeSet<(u32, IpAddr, u16)> {
    service
        .endpoints
        .iter()
        .flat_map(|endpoint| {
            endpoint.bind_addresses.iter().map(|address| {
                (
                    service.process.identity.pid,
                    address.parse().expect("bind address is an IP"),
                    endpoint.port,
                )
            })
        })
        .collect()
}

fn manifest_binds(fixture: &Fixture) -> BTreeSet<(u32, IpAddr, u16)> {
    fixture
        .record()
        .endpoints
        .iter()
        .map(|endpoint| (fixture.pid(), endpoint.ip().to_canonical(), endpoint.port()))
        .collect()
}

fn assert_matches_manifest_and_lsof(snapshot: &Observation, fixture: &Fixture) -> Service {
    let service = service_of(snapshot, fixture).clone();
    let record = fixture.record();

    let observed = observed_binds(&service);
    assert_eq!(observed, manifest_binds(fixture), "snapshot vs manifest");
    let ground_truth = lsof::tcp_listeners(&[fixture.pid()]).expect("lsof");
    assert_eq!(observed, ground_truth, "snapshot vs lsof");

    // One endpoint per port, each address once.
    let ports: BTreeSet<u16> = record
        .endpoints
        .iter()
        .map(std::net::SocketAddr::port)
        .collect();
    assert_eq!(
        service.endpoints.len(),
        ports.len(),
        "{:?}",
        service.endpoints
    );
    for endpoint in &service.endpoints {
        let distinct: BTreeSet<&String> = endpoint.bind_addresses.iter().collect();
        assert_eq!(distinct.len(), endpoint.bind_addresses.len());
    }

    assert_eq!(
        service.process.identity.started_at_secs,
        Some(record.started_at_secs)
    );
    assert_eq!(
        service.process.cwd.as_deref(),
        record.cwd.to_str(),
        "cwd with spaces and Unicode"
    );
    assert!(service.process.name.is_some());
    assert!(service.process.user_id.is_some());
    assert!(service.process.parent_pid.is_some());
    let file_name = record
        .executable
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 fixture file name");
    assert!(
        service
            .process
            .executable
            .as_deref()
            .is_some_and(|path| path.ends_with(&format!("/{file_name}"))),
        "executable {:?}",
        service.process.executable
    );
    for warning in &snapshot.warnings {
        match warning {
            ScanWarning::ProcessExited { pid, .. }
            | ScanWarning::MetadataUnavailable { pid, .. }
            | ScanWarning::AttributionUnavailable { pid, .. } => {
                assert_ne!(*pid, fixture.pid(), "unexpected warning {warning}");
            }
            ScanWarning::OtherUsersNotVisible => {}
        }
    }
    service
}

#[test]
fn fixture_listeners_match_manifest_and_lsof() {
    let ipv4 = spawn(&FixtureSpec::new().listen(&[V4]));
    let ipv6_only = spawn(&FixtureSpec::new().listen(&[V6]));
    let dual_loopback = spawn(&FixtureSpec::new().listen(&[V4, V6]));
    let multi_port = spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .listen(&[V4, V6])
            .listen(&[V6]),
    );
    let mapped =
        spawn(&FixtureSpec::new().listen(&[IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped())]));

    let snapshot = Scanner::new().scan().expect("scan");

    let service = assert_matches_manifest_and_lsof(&snapshot, &ipv4);
    assert_eq!(service.endpoints[0].bind_addresses, ["127.0.0.1"]);
    assert_eq!(service.process.name.as_deref(), Some("stackscan-fixture"));

    let service = assert_matches_manifest_and_lsof(&snapshot, &ipv6_only);
    assert_eq!(service.endpoints[0].bind_addresses, ["::1"]);
    assert_eq!(service.endpoints[0].exposure, Exposure::Loopback);

    let service = assert_matches_manifest_and_lsof(&snapshot, &dual_loopback);
    assert_eq!(
        service.endpoints.len(),
        1,
        "two sockets, one port, one endpoint"
    );
    assert_eq!(service.endpoints[0].bind_addresses, ["127.0.0.1", "::1"]);

    let service = assert_matches_manifest_and_lsof(&snapshot, &multi_port);
    assert_eq!(service.endpoints.len(), 3);

    let service = assert_matches_manifest_and_lsof(&snapshot, &mapped);
    assert_eq!(service.endpoints[0].bind_addresses, ["127.0.0.1"]);
}

/// macOS asks the user whether an unsigned binary may accept incoming connections when the
/// application firewall is on; a wildcard bind must never pop that dialog during tests.
fn application_firewall_off() -> bool {
    Command::new("/usr/libexec/ApplicationFirewall/socketfilterfw")
        .arg("--getglobalstate")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("State = 0"))
}

#[test]
fn dual_stack_wildcard_is_reported_as_all_interfaces() {
    if !application_firewall_off() {
        eprintln!(
            "skipped: the application firewall is on or unknown; a wildcard bind could prompt"
        );
        return;
    }
    let wildcard = spawn(&FixtureSpec::new().listen(&[IpAddr::V6(Ipv6Addr::UNSPECIFIED)]));
    let snapshot = Scanner::new().scan().expect("scan");
    let service = assert_matches_manifest_and_lsof(&snapshot, &wildcard);
    assert_eq!(service.endpoints[0].bind_addresses, ["::"]);
    assert_eq!(service.endpoints[0].exposure, Exposure::AllInterfaces);
}

/// The fixture's sockets from a real enumeration, and when that enumeration started.
fn sockets_of(fixture: &Fixture) -> (Vec<ListenSocket>, SystemTime) {
    let enumerated_at = SystemTime::now();
    let sockets: Vec<ListenSocket> = tcp_listen_sockets()
        .expect("enumerate sockets")
        .into_iter()
        .filter(|socket| socket.pid == fixture.pid())
        .collect();
    assert_eq!(sockets.len(), fixture.record().endpoints.len());
    (sockets, enumerated_at)
}

fn assert_reported_as_exited(snapshot: &Observation, fixture: &Fixture) {
    assert!(
        snapshot
            .services
            .iter()
            .all(|service| service.process.identity.pid != fixture.pid()),
        "an exited process is not presented as a live service"
    );
    let port = fixture.record().endpoints[0].port();
    assert!(
        snapshot.warnings.contains(&ScanWarning::ProcessExited {
            pid: fixture.pid(),
            ports: vec![port],
        }),
        "missing exit warning for fixture {}",
        fixture.pid()
    );
}

#[test]
fn process_exiting_between_enumeration_and_inspection_is_a_warning() {
    let mut fixture = spawn(&FixtureSpec::new().listen(&[V4]));
    let (sockets, enumerated_at) = sockets_of(&fixture);

    fixture.kill().expect("kill owned fixture");
    fixture.wait(Duration::from_secs(5)).expect("reap");

    let snapshot = Scanner::new().observation_of(sockets, enumerated_at);
    assert_reported_as_exited(&snapshot, &fixture);
}

#[test]
fn zombie_between_enumeration_and_inspection_is_a_warning() {
    let mut fixture = spawn(&FixtureSpec::new().listen(&[V4]));
    let (sockets, enumerated_at) = sockets_of(&fixture);

    // Killed but not yet reaped: the PID still exists as a zombie (`ps` shows `Z`) with no
    // sockets. sysinfo either omits it or reports Zombie; both must read as exited.
    fixture.kill().expect("kill owned fixture");
    let mut inspector = ProcessInspector::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while inspector.inspect(&[fixture.pid()]).get(&fixture.pid()) != Some(&ProcessState::Exited) {
        assert!(Instant::now() < deadline, "fixture never became a zombie");
        thread::sleep(Duration::from_millis(10));
    }

    let snapshot = Scanner::new().observation_of(sockets, enumerated_at);
    assert_reported_as_exited(&snapshot, &fixture);
    fixture.wait(Duration::from_secs(5)).expect("reap");
}

#[test]
fn process_started_after_enumeration_is_not_the_socket_owner() {
    // Deterministic stand-in for PID reuse: the live process at this PID started after the
    // (injected) enumeration time, so it cannot be the process whose socket was seen.
    let fixture = spawn(&FixtureSpec::new().listen(&[V4]));
    let (sockets, _) = sockets_of(&fixture);
    let before_fixture_started =
        SystemTime::UNIX_EPOCH + Duration::from_secs(fixture.record().started_at_secs - 5);

    let snapshot = Scanner::new().observation_of(sockets, before_fixture_started);
    assert_reported_as_exited(&snapshot, &fixture);
}

#[test]
fn truncated_non_utf8_process_name_keeps_the_socket() {
    // The kernel keeps 31 bytes of a process name: 28 ASCII bytes plus "éééé" is cut inside
    // the second "é". Unpatched `listeners` 0.6.1 then dropped every socket of the process.
    let name = format!("{}éééé", "a".repeat(28));
    let fixture = spawn(&FixtureSpec::new().listen(&[V4]).executable_name(&name));

    let snapshot = Scanner::new().scan().expect("scan");
    let service = assert_matches_manifest_and_lsof(&snapshot, &fixture);
    let reported = service
        .process
        .name
        .expect("a lossy name, not a dropped socket");
    assert_eq!(reported, format!("{}é\u{FFFD}", "a".repeat(28)));
}

fn running_as_root() -> bool {
    Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .is_ok_and(|output| output.stdout.trim_ascii() == b"0")
}

#[test]
fn hidden_sockets_of_other_users_are_a_warning() {
    let snapshot = Scanner::new().scan().expect("scan");
    let warned = snapshot
        .warnings
        .contains(&ScanWarning::OtherUsersNotVisible);
    assert_eq!(warned, !running_as_root());
}

#[test]
fn withheld_metadata_is_unknown_not_guessed() {
    if running_as_root() {
        eprintln!("skipped: root may read every process's metadata");
        return;
    }
    // launchd (PID 1, root) is real and always present; an unprivileged caller may not read its
    // start time, owner or cwd. Its sockets are hidden from us, so this one socket is injected.
    let socket = ListenSocket {
        pid: 1,
        address: V4,
        port: 1,
        process_name: Some("launchd".into()),
        executable: None,
    };
    let snapshot = Scanner::new().observation_of(vec![socket], SystemTime::now());

    let service = snapshot
        .services
        .iter()
        .find(|service| service.process.identity.pid == 1)
        .unwrap_or_else(|| panic!("a live process with gaps stays a service"));
    assert_eq!(service.process.identity.started_at_secs, None);
    assert_eq!(service.process.user_id, None);
    assert_eq!(service.process.cwd, None);
    assert_eq!(service.process.command, None);
    assert_eq!(service.process.metrics.cpu, CpuUsage::Unavailable);
    assert_eq!(service.process.metrics.memory_bytes, None);
    assert_eq!(service.project, None, "no cwd, no guessed project");
    assert!(
        snapshot
            .warnings
            .contains(&ScanWarning::MetadataUnavailable {
                pid: 1,
                fields: vec![
                    ProcessField::StartTime,
                    ProcessField::User,
                    ProcessField::Command,
                    ProcessField::Cwd,
                    ProcessField::Cpu,
                    ProcessField::Memory,
                ],
            })
    );
}

#[test]
fn snapshot_serializes_with_the_contract_field_names() {
    let fixture = spawn(&FixtureSpec::new().listen(&[V4, V6]));
    let snapshot = Scanner::new().scan().expect("scan");
    let json = serde_json::to_value(service_of(&snapshot, &fixture)).expect("serialize");
    let endpoint = &json["endpoints"][0];
    assert_eq!(endpoint["protocol"], "tcp");
    assert_eq!(endpoint["exposure"], "loopback");
    assert_eq!(
        endpoint["bindAddresses"],
        serde_json::json!(["127.0.0.1", "::1"])
    );
    assert_eq!(
        json["process"]["identity"]["startedAtSecs"],
        fixture.record().started_at_secs
    );
    let warning = serde_json::to_value(ScanWarning::ProcessExited {
        pid: 7,
        ports: vec![80],
    })
    .expect("serialize");
    assert_eq!(
        warning,
        serde_json::json!({"kind": "processExited", "pid": 7, "ports": [80]})
    );
}

#[test]
fn repeated_pids_are_inspected_once_and_kept() {
    // sysinfo 0.39 drops a process that appears twice in one refresh; the inspector dedupes.
    let fixture = spawn(&FixtureSpec::new().listen(&[V4]));
    let states = ProcessInspector::new().inspect(&[fixture.pid(), fixture.pid()]);
    assert_eq!(states.len(), 1);
    let Some(ProcessState::Running(details)) = states.get(&fixture.pid()) else {
        panic!("fixture {} must be running: {states:?}", fixture.pid());
    };
    assert_eq!(
        details.started_at_secs,
        Some(fixture.record().started_at_secs)
    );
}
