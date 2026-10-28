//! Scan cost with 30 owned fixture listeners, against the 3-second cadence target in
//! blueprint/architecture.md. Run with `--no-capture` to see the measurements.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant, SystemTime},
};

use stackscan_core::{scan::Scanner, sockets::tcp_listen_sockets};
use stackscan_fixtures::{Fixture, FixtureSpec, lsof};

const FIXTURES: usize = 30;
const SCANS: usize = 20;
const CADENCE: Duration = Duration::from_secs(3);
/// Regression guard: 10% of the cadence. Measured medians are ~2 ms (release) and ~4 ms
/// (debug), so this leaves CI runners two orders of magnitude of headroom.
const BUDGET: Duration = Duration::from_millis(300);

fn stats(mut samples: Vec<f64>) -> (f64, f64, f64) {
    samples.sort_by(f64::total_cmp);
    (
        samples[0],
        samples[samples.len() / 2],
        samples[samples.len() - 1],
    )
}

#[test]
fn thirty_listener_scan_stays_well_under_one_interval() {
    let spec = FixtureSpec::new().listen(&[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    let fixtures: Vec<Fixture> = (0..FIXTURES)
        .map(|_| Fixture::spawn(&spec).expect("spawn fixture"))
        .collect();
    let pids: Vec<u32> = fixtures.iter().map(Fixture::pid).collect();

    let mut scanner = Scanner::new();
    scanner.scan().expect("warm-up scan");

    let mut totals = Vec::with_capacity(SCANS);
    let mut enumerations = Vec::with_capacity(SCANS);
    let mut inspections = Vec::with_capacity(SCANS);
    let mut listener_count = 0;
    for _ in 0..SCANS {
        let snapshot = scanner.scan().expect("scan");
        let seen = snapshot
            .services
            .iter()
            .filter(|service| pids.contains(&service.process.identity.pid))
            .count();
        assert_eq!(seen, FIXTURES, "every fixture in every scan");
        totals.push(snapshot.scan_duration_ms);

        let started = Instant::now();
        let enumerated_at = SystemTime::now();
        let sockets = tcp_listen_sockets().expect("enumerate");
        enumerations.push(started.elapsed().as_secs_f64() * 1000.0);
        listener_count = sockets.len();
        inspections.push(scanner.snapshot_of(sockets, enumerated_at).scan_duration_ms);
    }

    let started = Instant::now();
    let ground_truth = lsof::tcp_listeners(&pids).expect("lsof");
    let lsof_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(ground_truth.len(), FIXTURES);

    let (min, median, max) = stats(totals);
    let (_, enumerate_median, _) = stats(enumerations);
    let (_, inspect_median, _) = stats(inspections);
    eprintln!(
        "scan cost over {SCANS} scans with {FIXTURES} fixture listeners \
         ({listener_count} TCP listeners visible in total): \
         min {min:.2} ms, median {median:.2} ms, max {max:.2} ms; \
         median enumeration {enumerate_median:.2} ms, median inspection {inspect_median:.2} ms; \
         lsof for the fixture PIDs alone {lsof_ms:.1} ms"
    );
    assert!(
        median < BUDGET.as_secs_f64() * 1000.0,
        "median scan {median:.1} ms exceeds {BUDGET:?} (10% of the {CADENCE:?} cadence)"
    );
}
