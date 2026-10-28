//! Scan cost with 30 owned fixture listeners spread over 10 Git worktrees (3 packages each),
//! with attribution and metrics, against the 3-second cadence target in
//! blueprint/architecture.md. Run with `--no-capture` to see the measurements.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant, SystemTime},
};

use kosse_core::{scan::Scanner, sockets::tcp_listen_sockets};
use kosse_fixtures::{
    Fixture, FixtureSpec, lsof,
    repo::{Sandbox, git, init_repo},
};

const WORKTREES: usize = 10;
const PACKAGES: usize = 3;
const FIXTURES: usize = WORKTREES * PACKAGES;
const SCANS: usize = 20;
const CADENCE: Duration = Duration::from_secs(3);
/// Regression guard: 10% of the cadence. Measured warm medians are a few milliseconds, so this
/// leaves CI runners two orders of magnitude of headroom.
const BUDGET: Duration = Duration::from_millis(300);
/// The first scan also runs `git` for every worktree.
const COLD_BUDGET: Duration = Duration::from_secs(1);

fn stats(mut samples: Vec<f64>) -> (f64, f64, f64) {
    samples.sort_by(f64::total_cmp);
    (
        samples[0],
        samples[samples.len() / 2],
        samples[samples.len() - 1],
    )
}

/// Five repositories, each with a main and a linked worktree, three packages in each.
fn worktree_fixtures(tree: &Sandbox) -> Vec<Fixture> {
    let mut roots = Vec::new();
    for repo in 0..WORKTREES / 2 {
        let main = tree.dir(&format!("repo {repo}/main")).expect("dir");
        init_repo(&main).expect("repo");
        for package in 0..PACKAGES {
            tree.file(
                &format!("repo {repo}/main/pkg-{package}/package.json"),
                "{}",
            )
            .expect("manifest");
        }
        git(&main, &["add", "."]).expect("add");
        git(&main, &["commit", "-q", "-m", "packages"]).expect("commit");
        let linked = tree.root().join(format!("repo {repo}/linked"));
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                linked.to_str().expect("path"),
            ],
        )
        .expect("worktree");
        roots.extend([main, linked]);
    }
    let spec = FixtureSpec::new().listen(&[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    roots
        .iter()
        .flat_map(|root| (0..PACKAGES).map(move |package| root.join(format!("pkg-{package}"))))
        .map(|dir| Fixture::spawn(&spec.clone().cwd(&dir)).expect("spawn fixture"))
        .collect()
}

/// rev-parse + symbolic-ref once per worktree, not per process or port, then nothing while the
/// cache is fresh.
fn assert_fixture_git_runs(pids: &[u32]) {
    let fixture_sockets = || {
        let enumerated_at = SystemTime::now();
        let sockets: Vec<_> = tcp_listen_sockets()
            .expect("enumerate")
            .into_iter()
            .filter(|socket| pids.contains(&socket.pid))
            .collect();
        (sockets, enumerated_at)
    };
    let mut scanner = Scanner::new();
    let (sockets, enumerated_at) = fixture_sockets();
    scanner.observation_of(sockets, enumerated_at);
    assert_eq!(scanner.stats().git_commands, 2 * WORKTREES as u64);
    for _ in 0..SCANS {
        let (sockets, enumerated_at) = fixture_sockets();
        scanner.observation_of(sockets, enumerated_at);
    }
    assert_eq!(
        scanner.stats().git_commands,
        2 * WORKTREES as u64,
        "warm scans reuse cached attribution"
    );
}

#[test]
fn thirty_listener_scan_stays_well_under_one_interval() {
    let tree = Sandbox::new().expect("sandbox");
    let fixtures = worktree_fixtures(&tree);
    let pids: Vec<u32> = fixtures.iter().map(Fixture::pid).collect();

    let mut scanner = Scanner::new();
    let cold = scanner.scan().expect("cold scan");
    let cold_git = scanner.stats().git_commands;
    let attributed = cold
        .services
        .iter()
        .filter(|service| pids.contains(&service.process.identity.pid))
        .filter(|service| service.project.is_some() && service.package.is_some())
        .count();
    assert_eq!(attributed, FIXTURES, "every fixture attributed");
    let fixture_projects = cold
        .projects
        .iter()
        .filter(|project| {
            project
                .root
                .starts_with(tree.root().to_str().expect("path"))
        })
        .count();
    assert_eq!(fixture_projects, WORKTREES, "one project per worktree");
    // The machine's own services add their worktrees' git runs to full scans, and may restart at
    // any time, so exact git counts are checked on the fixtures' sockets alone.
    assert_fixture_git_runs(&pids);

    let mut totals = Vec::with_capacity(SCANS);
    let mut enumerations = Vec::with_capacity(SCANS);
    let mut assemblies = Vec::with_capacity(SCANS);
    let mut listener_count = 0;
    for _ in 0..SCANS {
        let observation = scanner.scan().expect("scan");
        let seen = observation
            .services
            .iter()
            .filter(|service| pids.contains(&service.process.identity.pid))
            .count();
        assert_eq!(seen, FIXTURES, "every fixture in every scan");
        totals.push(observation.scan_duration_ms);

        let started = Instant::now();
        let enumerated_at = SystemTime::now();
        let sockets = tcp_listen_sockets().expect("enumerate");
        enumerations.push(started.elapsed().as_secs_f64() * 1000.0);
        listener_count = sockets.len();
        assemblies.push(
            scanner
                .observation_of(sockets, enumerated_at)
                .scan_duration_ms,
        );
    }
    let warm_git = scanner.stats().git_commands - cold_git;

    let started = Instant::now();
    let ground_truth = lsof::tcp_listeners(&pids).expect("lsof");
    let lsof_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(ground_truth.len(), FIXTURES);

    let (min, median, max) = stats(totals);
    let (_, enumerate_median, _) = stats(enumerations);
    let (_, assemble_median, _) = stats(assemblies);
    eprintln!(
        "cold scan {:.2} ms with {cold_git} git commands; warm scans over {SCANS} with \
         {FIXTURES} fixture listeners in {WORKTREES} worktrees ({listener_count} TCP listeners \
         visible in total): min {min:.2} ms, median {median:.2} ms, max {max:.2} ms; median \
         enumeration {enumerate_median:.2} ms, median inspection+attribution+metrics \
         {assemble_median:.2} ms; {warm_git} git commands while warm; lsof for the fixture PIDs \
         alone {lsof_ms:.1} ms (git counts include this machine's own services)",
        cold.scan_duration_ms
    );
    assert!(
        cold.scan_duration_ms < COLD_BUDGET.as_secs_f64() * 1000.0,
        "cold scan {:.1} ms exceeds {COLD_BUDGET:?}",
        cold.scan_duration_ms
    );
    assert!(
        median < BUDGET.as_secs_f64() * 1000.0,
        "median scan {median:.1} ms exceeds {BUDGET:?} (10% of the {CADENCE:?} cadence)"
    );
}
