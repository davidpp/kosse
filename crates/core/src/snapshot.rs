//! Turns scan attempts into published [`Snapshot`]s: a monotonic revision, the last good
//! observation kept across failures, and the scanner's failure and overrun record.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    model::{Observation, SNAPSHOT_SCHEMA_VERSION, ScannerStatus, Snapshot},
    scan::unix_millis,
};

/// One instance's publication history. Owned by whoever runs the scanner.
#[derive(Debug)]
pub struct SnapshotTracker {
    instance: String,
    interval: Option<Duration>,
    revision: u64,
    last_good: Option<Observation>,
    consecutive_failures: u32,
    overruns: u64,
}

/// How one attempt went.
#[derive(Debug, Clone, Copy)]
pub struct Attempt {
    /// Unix milliseconds at the start of the attempt.
    pub started_at_ms: u64,
    pub duration: Duration,
}

/// Measures one attempt from both clocks: wall time for the record, monotonic for the duration.
#[derive(Debug, Clone, Copy)]
pub struct AttemptTimer {
    started_at_ms: u64,
    started: Instant,
}

impl AttemptTimer {
    #[must_use]
    pub fn start() -> Self {
        Self {
            started_at_ms: unix_millis(SystemTime::now()),
            started: Instant::now(),
        }
    }

    #[must_use]
    pub fn finish(self) -> Attempt {
        Attempt {
            started_at_ms: self.started_at_ms,
            duration: self.started.elapsed(),
        }
    }
}

impl SnapshotTracker {
    /// `interval` is the target time between scans; `None` for a one-shot scan.
    #[must_use]
    pub fn new(interval: Option<Duration>) -> Self {
        Self {
            instance: instance_id(),
            interval,
            revision: 0,
            last_good: None,
            consecutive_failures: 0,
            overruns: 0,
        }
    }

    pub fn succeeded(&mut self, observation: Observation, attempt: Attempt) -> Snapshot {
        self.last_good = Some(observation);
        self.consecutive_failures = 0;
        self.publish(attempt, None)
    }

    /// Keeps the last good observation; the failure is reported next to it.
    pub fn failed(&mut self, failure: String, attempt: Attempt) -> Snapshot {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.publish(attempt, Some(failure))
    }

    /// Whether an attempt of this length overran the interval.
    #[must_use]
    pub fn overran(&self, duration: Duration) -> bool {
        self.interval.is_some_and(|interval| duration > interval)
    }

    fn publish(&mut self, attempt: Attempt, failure: Option<String>) -> Snapshot {
        if self.overran(attempt.duration) {
            self.overruns += 1;
        }
        self.revision += 1;
        Snapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            instance: self.instance.clone(),
            revision: self.revision,
            observation: self.last_good.clone(),
            scanner: ScannerStatus {
                interval_ms: self
                    .interval
                    .map(|interval| u64::try_from(interval.as_millis()).unwrap_or(u64::MAX)),
                last_attempt_at_ms: attempt.started_at_ms,
                last_attempt_duration_ms: attempt.duration.as_secs_f64() * 1000.0,
                failure,
                consecutive_failures: self.consecutive_failures,
                overruns: self.overruns,
            },
        }
    }
}

/// This process's ID, its start instant and a per-process sequence (one tracker is one
/// publisher): unique per run on one machine, opaque to readers.
fn instance_id() -> String {
    static TRACKERS: AtomicU64 = AtomicU64::new(0);
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let sequence = TRACKERS.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{started:x}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(scanned_at_ms: u64) -> Observation {
        Observation {
            scanned_at_ms,
            scan_duration_ms: 1.0,
            services: Vec::new(),
            projects: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn attempt(started_at_ms: u64, millis: u64) -> Attempt {
        Attempt {
            started_at_ms,
            duration: Duration::from_millis(millis),
        }
    }

    #[test]
    fn failure_keeps_the_last_good_observation_and_says_why() {
        let mut tracker = SnapshotTracker::new(Some(Duration::from_secs(3)));
        let good = tracker.succeeded(observation(1_000), attempt(1_000, 2));
        let failed = tracker.failed("enumeration failed".into(), attempt(4_000, 1));
        let again = tracker.failed("enumeration failed".into(), attempt(7_000, 1));

        assert_eq!(failed.observation, good.observation);
        assert_eq!(again.observation, good.observation);
        assert_eq!(again.scanner.failure.as_deref(), Some("enumeration failed"));
        assert_eq!(again.scanner.consecutive_failures, 2);
        assert_eq!(again.scanner.last_attempt_at_ms, 7_000);

        let recovered = tracker.succeeded(observation(10_000), attempt(10_000, 2));
        assert_eq!(recovered.scanner.failure, None);
        assert_eq!(recovered.scanner.consecutive_failures, 0);
        assert_eq!(
            recovered
                .observation
                .map(|observation| observation.scanned_at_ms),
            Some(10_000)
        );
    }

    #[test]
    fn initial_failure_has_no_observation_rather_than_an_empty_one() {
        let mut tracker = SnapshotTracker::new(Some(Duration::from_secs(3)));
        let snapshot = tracker.failed("enumeration failed".into(), attempt(1_000, 1));
        assert_eq!(snapshot.observation, None);
        assert!(snapshot.scanner.failure.is_some());
    }

    #[test]
    fn revisions_increase_with_every_publication() {
        let mut tracker = SnapshotTracker::new(Some(Duration::from_secs(3)));
        let revisions = [
            tracker.succeeded(observation(1), attempt(1, 1)).revision,
            tracker.failed("x".into(), attempt(2, 1)).revision,
            tracker.succeeded(observation(3), attempt(3, 1)).revision,
        ];
        assert_eq!(revisions, [1, 2, 3]);
    }

    #[test]
    fn instances_are_told_apart_and_stable_within_one() {
        let mut first = SnapshotTracker::new(None);
        let mut second = SnapshotTracker::new(None);
        let a = first.succeeded(observation(1), attempt(1, 1));
        let b = first.succeeded(observation(2), attempt(2, 1));
        let c = second.succeeded(observation(3), attempt(3, 1));
        assert_eq!(a.instance, b.instance);
        assert_ne!(
            a.instance, c.instance,
            "a restart is not an out-of-order event"
        );
        assert_eq!(c.revision, 1);
    }

    #[test]
    fn attempts_longer_than_the_interval_count_as_overruns() {
        let mut tracker = SnapshotTracker::new(Some(Duration::from_secs(3)));
        tracker.succeeded(observation(1), attempt(1, 2_999));
        let overrun = tracker.succeeded(observation(2), attempt(2, 3_001));
        assert_eq!(overrun.scanner.overruns, 1);
        assert_eq!(overrun.scanner.interval_ms, Some(3_000));

        let mut one_shot = SnapshotTracker::new(None);
        let snapshot = one_shot.succeeded(observation(1), attempt(1, 60_000));
        assert_eq!(snapshot.scanner.overruns, 0);
        assert_eq!(snapshot.scanner.interval_ms, None);
        assert_eq!(snapshot.revision, 1);
    }
}
