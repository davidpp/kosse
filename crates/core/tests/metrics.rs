//! CPU and memory of real owned fixtures: a busy one (one spinning thread) and an idle one.
#![expect(clippy::panic, reason = "test helpers fail the test by panicking")]

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    thread,
    time::{Duration, SystemTime},
};

use kosse_core::{
    model::{CpuUsage, Observation, ProcessMetrics},
    scan::Scanner,
    sockets::tcp_listen_sockets,
};
use kosse_fixtures::{Fixture, FixtureSpec};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
/// The measurement window between the two scans, not a correctness wait.
const WINDOW: Duration = Duration::from_secs(1);

fn metrics_of(observation: &Observation, fixture: &Fixture) -> ProcessMetrics {
    observation
        .services
        .iter()
        .find(|service| service.process.identity.pid == fixture.pid())
        .unwrap_or_else(|| panic!("fixture {} missing", fixture.pid()))
        .process
        .metrics
}

fn percent(usage: CpuUsage) -> f64 {
    match usage {
        CpuUsage::Sampled { percent } => percent,
        other => panic!("expected a sample, got {other:?}"),
    }
}

#[test]
fn cpu_warms_up_then_samples_a_controlled_workload() {
    let busy = Fixture::spawn(&FixtureSpec::new().listen(&[V4]).busy()).expect("busy fixture");
    let idle = Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("idle fixture");
    let mut scanner = Scanner::new();

    let first = scanner.scan().expect("scan");
    for fixture in [&busy, &idle] {
        let metrics = metrics_of(&first, fixture);
        assert_eq!(metrics.cpu, CpuUsage::WarmingUp, "never a made-up zero");
        assert!(metrics.memory_bytes.is_some_and(|bytes| bytes > 0));
    }

    thread::sleep(WINDOW);
    let second = scanner.scan().expect("scan");
    let busy_percent = percent(metrics_of(&second, &busy).cpu);
    let idle_percent = percent(metrics_of(&second, &idle).cpu);
    eprintln!("busy fixture {busy_percent} % of one core, idle fixture {idle_percent} %");
    // One spinning thread is ~100 %; the bounds leave room for a loaded CI runner.
    assert!(
        (40.0..=150.0).contains(&busy_percent),
        "busy {busy_percent}"
    );
    assert!(idle_percent < 5.0, "idle {idle_percent}");
}

#[test]
fn each_process_is_inspected_once_per_scan_however_many_ports() {
    let fixtures: Vec<Fixture> = (0..3)
        .map(|_| {
            Fixture::spawn(
                &FixtureSpec::new()
                    .listen(&[V4, V6])
                    .listen(&[V4])
                    .listen(&[V6]),
            )
            .expect("spawn fixture")
        })
        .collect();
    let pids: Vec<u32> = fixtures.iter().map(Fixture::pid).collect();
    let enumerated_at = SystemTime::now();
    let sockets: Vec<_> = tcp_listen_sockets()
        .expect("enumerate")
        .into_iter()
        .filter(|socket| pids.contains(&socket.pid))
        .collect();
    assert_eq!(sockets.len(), 12, "four sockets per fixture");

    let mut scanner = Scanner::new();
    let before = scanner.stats().processes_inspected;
    let observation = scanner.observation_of(sockets, enumerated_at);
    assert_eq!(observation.services.len(), 3);
    assert_eq!(
        scanner.stats().processes_inspected - before,
        3,
        "12 sockets, 3 processes, 3 inspections"
    );
    assert_eq!(
        scanner.stats().retained_processes,
        3,
        "only this scan's processes stay"
    );
}

#[test]
fn retained_process_table_shrinks_when_listeners_go_away() {
    let mut fixtures: Vec<Fixture> = (0..4)
        .map(|_| Fixture::spawn(&FixtureSpec::new().listen(&[V4])).expect("spawn fixture"))
        .collect();
    let pids: Vec<u32> = fixtures.iter().map(Fixture::pid).collect();
    let mut scanner = Scanner::new();
    let scan_fixtures = |scanner: &mut Scanner| {
        let enumerated_at = SystemTime::now();
        let sockets: Vec<_> = tcp_listen_sockets()
            .expect("enumerate")
            .into_iter()
            .filter(|socket| pids.contains(&socket.pid))
            .collect();
        scanner.observation_of(sockets, enumerated_at)
    };
    scan_fixtures(&mut scanner);
    assert_eq!(scanner.stats().retained_processes, 4);

    for fixture in &mut fixtures[..3] {
        fixture.stop().expect("stop owned fixture");
    }
    // The first scan after the exits drops them; the table never keeps what no scan reports.
    let observation = scan_fixtures(&mut scanner);
    assert_eq!(observation.services.len(), 1);
    scan_fixtures(&mut scanner);
    assert!(scanner.stats().retained_processes <= 1);
}
