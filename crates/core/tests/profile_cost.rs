//! What project profiles add to a scan: 30 owned fixture listeners in 10 repositories, each with
//! a central profile and a repository `kosse.json` that name all three of its packages' services.
//! Run with `--no-capture` to see the measurements.

use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

use kosse_core::{
    profiles::{CentralStore, Profiles, ProjectProfile, view::ProfileMatch},
    scan::Scanner,
};
use kosse_fixtures::{
    Fixture, FixtureSpec,
    repo::{Sandbox, init_repo},
};
use serde_json::json;

const REPOSITORIES: usize = 10;
const PACKAGES: usize = 3;
const FIXTURES: usize = REPOSITORIES * PACKAGES;
const SCANS: usize = 20;
/// Regression guard: 1% of the 3-second cadence.
const BUDGET: Duration = Duration::from_millis(30);

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

#[test]
fn profiles_add_little_to_a_thirty_listener_scan() {
    let tree = Sandbox::new().expect("sandbox");
    let store = CentralStore::new(tree.root().join("config"));
    let spec = FixtureSpec::new().listen(&[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    let mut fixtures = Vec::new();
    for repository in 0..REPOSITORIES {
        let root = tree.dir(&format!("repo-{repository}")).expect("dir");
        init_repo(&root).expect("repo");
        let services: serde_json::Map<String, serde_json::Value> = (0..PACKAGES)
            .map(|package| {
                let entry = json!({
                    "name": format!("Package {package}"),
                    "match": { "cwd": format!("pkg-{package}") },
                    "protocol": "http",
                    "links": [{ "label": "Local", "url": format!("http://pkg-{package}.localhost/") }],
                });
                (format!("pkg-{package}"), entry)
            })
            .collect();
        let profile = json!({ "version": 1, "services": services });
        fs::write(root.join("kosse.json"), profile.to_string()).expect("repo file");
        let central: ProjectProfile =
            serde_json::from_value(json!({ "version": 1, "name": format!("Repo {repository}") }))
                .expect("profile");
        store
            .save(root.to_str().expect("UTF-8"), Some(central))
            .expect("save");
        for package in 0..PACKAGES {
            let dir = tree
                .dir(&format!("repo-{repository}/pkg-{package}"))
                .expect("dir");
            fixtures.push(Fixture::spawn(&spec.clone().cwd(&dir)).expect("fixture"));
        }
    }
    let pids: Vec<u32> = fixtures.iter().map(Fixture::pid).collect();

    let mut scanner = Scanner::new();
    let mut profiles = Profiles::new(store);
    let mut cold = None;
    let mut applies = Vec::with_capacity(SCANS);
    let mut scans = Vec::with_capacity(SCANS);
    for _ in 0..=SCANS {
        let mut observation = scanner.scan().expect("scan");
        let started = Instant::now();
        profiles.apply(&mut observation, scanner.own_user());
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let labeled = observation
            .services
            .iter()
            .filter(|service| pids.contains(&service.process.identity.pid))
            .filter(|service| matches!(service.profile, Some(ProfileMatch::Matched { .. })))
            .count();
        assert_eq!(labeled, FIXTURES, "every fixture matched one service");
        if cold.is_none() {
            cold = Some(elapsed_ms);
        } else {
            applies.push(elapsed_ms);
            scans.push(observation.scan_duration_ms);
        }
    }
    let cold = cold.unwrap_or_default();
    let (apply_median, scan_median) = (median(applies), median(scans));
    eprintln!(
        "profiles over {FIXTURES} fixture listeners in {REPOSITORIES} repositories: first apply \
         (reads {REPOSITORIES} repository files and the central file) {cold:.2} ms; warm median \
         {apply_median:.3} ms over {SCANS} scans, next to a median scan of {scan_median:.2} ms"
    );
    assert!(
        apply_median < BUDGET.as_secs_f64() * 1000.0,
        "median apply {apply_median:.2} ms exceeds {BUDGET:?}"
    );
}
