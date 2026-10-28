//! The harness itself, against real fixture processes: readiness, reachability, identity-checked
//! cleanup and the self-teardown backstops.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpStream},
    time::Duration,
};

use stackscan_fixtures::{Cleanup, Fixture, FixtureError, FixtureSpec, live_start_time, lsof};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

fn banner(address: std::net::SocketAddr) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).expect("connect");
    let mut text = String::new();
    stream.read_to_string(&mut text).expect("read banner");
    text
}

#[test]
fn spawned_fixture_is_listening_and_recorded() {
    let fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[V4, V6]).listen(&[V4])).expect("spawn fixture");
    let record = fixture.record();

    assert_eq!(record.endpoints.len(), 3);
    assert_eq!(record.endpoints[0].port(), record.endpoints[1].port());
    assert_ne!(record.endpoints[0].port(), record.endpoints[2].port());
    assert_eq!(live_start_time(record.pid), Some(record.started_at_secs));
    assert!(record.cwd.is_dir(), "fixture cwd exists while it runs");
    assert_eq!(record.cleanup, Cleanup::Running);
    for &endpoint in &record.endpoints {
        assert_eq!(banner(endpoint), "stackscan-fixture\n");
    }

    let expected: Vec<_> = record
        .endpoints
        .iter()
        .map(|endpoint| (record.pid, endpoint.ip(), endpoint.port()))
        .collect();
    let mut expected_sorted = expected.clone();
    expected_sorted.sort_unstable();
    let ground_truth: Vec<_> = lsof::tcp_listeners(&[record.pid])
        .expect("lsof")
        .into_iter()
        .collect();
    assert_eq!(ground_truth, expected_sorted);
}

#[test]
fn stop_closes_stdin_and_reaps() {
    let mut fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("spawn fixture");
    let pid = fixture.pid();
    let cwd = fixture.record().cwd.clone();

    assert_eq!(fixture.stop().expect("stop"), Cleanup::Closed);
    assert_eq!(live_start_time(pid), None);
    assert_eq!(fixture.record().cleanup, Cleanup::Closed);
    drop(fixture);
    assert!(!cwd.exists(), "fixture cwd removed with the fixture");
}

#[test]
fn mismatched_identity_is_refused_and_leaves_the_process_alone() {
    let mut fixture = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("spawn fixture");
    let endpoint = fixture.record().endpoints[0];
    let real_start = fixture.record().started_at_secs;

    // Injected mismatch: the record now claims a different process start.
    fixture.record_mut().started_at_secs = real_start - 1;
    assert!(matches!(
        fixture.stop(),
        Err(FixtureError::IdentityMismatch { expected, found: Some(found), .. })
            if expected == real_start - 1 && found == real_start
    ));
    assert!(matches!(
        fixture.kill(),
        Err(FixtureError::IdentityMismatch { .. })
    ));
    assert_eq!(banner(endpoint), "stackscan-fixture\n", "still serving");
    assert_eq!(fixture.record().cleanup, Cleanup::Running);

    fixture.record_mut().started_at_secs = real_start;
    assert_eq!(fixture.stop().expect("stop"), Cleanup::Closed);
}

#[test]
fn fixture_exits_by_itself_after_its_lifetime() {
    let mut fixture = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .max_lifetime(Duration::from_secs(1)),
    )
    .expect("spawn fixture");

    let status = fixture.wait(Duration::from_secs(10)).expect("self exit");
    assert!(status.success(), "{status}");
    assert_eq!(fixture.stop().expect("stop"), Cleanup::Exited);
}

#[test]
fn bind_failure_is_reported_with_the_fixture_error() {
    // TEST-NET-1 (RFC 5737) is never assigned to a local interface.
    let unassigned = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
    let Err(FixtureError::NotReady { reason, stderr }) =
        Fixture::spawn(&FixtureSpec::new().listen(&[unassigned]))
    else {
        panic!("binding an unassigned address must fail");
    };
    assert_eq!(reason, "exited before ready");
    assert!(stderr.contains("could not bind"), "{stderr}");
}
