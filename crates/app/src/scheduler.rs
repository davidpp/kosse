//! The application's one scanner loop.
//!
//! A dedicated thread owns the only [`Scanner`] and its retained state, so scans are serialized
//! by construction and never run on async request workers. It scans on a fixed cadence and on
//! request; refresh requests that arrive while a scan is pending or running are coalesced into
//! the next scan, which starts after all of them. Every attempt publishes a [`Snapshot`] on a
//! `watch` channel (latest value only), keeping the last good observation when a scan fails.
//!
//! Panics are contained twice: a panicking scan (or scanner construction) is a failed attempt
//! and the scanner is rebuilt; a panic anywhere else in the loop publishes a final failure
//! snapshot, after which refreshes report that the scanner stopped.

use std::{
    io,
    panic::{self, AssertUnwindSafe},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use stackscan_core::{
    model::{Observation, Snapshot},
    scan::Scanner,
    snapshot::{AttemptTimer, SnapshotTracker},
};
use tokio::sync::{oneshot, watch};

/// Target time between scans (blueprint/architecture.md).
pub const SCAN_INTERVAL: Duration = Duration::from_secs(3);
/// Longest a [`SchedulerHandle::refresh`] waits. A scan normally takes milliseconds; the bound
/// covers a scan stuck on a stalled file system, so callers never hang with it.
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

/// The latest publication; `None` until the first attempt finishes.
pub type Latest = Option<Arc<Snapshot>>;

/// One scan attempt, as the worker sees it.
type Scan = Box<dyn FnMut() -> Result<Observation, String> + Send>;
/// Builds a fresh scan (with fresh retained state); called again after a panic.
type MakeScan = Box<dyn Fn() -> Scan + Send>;

enum Request {
    Refresh(oneshot::Sender<Arc<Snapshot>>),
    Shutdown,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "returned by refresh, which the S04 API calls")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RefreshError {
    #[error("the scanner has stopped")]
    Stopped,
    #[error("no scan finished within the refresh timeout")]
    TimedOut,
}

/// What request handlers (S04) and the tray (S08) hold: read the latest snapshot, wait for
/// changes, or ask for a fresh scan. Cloning is cheap; it never scans by itself.
#[derive(Clone)]
pub struct SchedulerHandle {
    requests: mpsc::Sender<Request>,
    latest: watch::Receiver<Latest>,
}

impl SchedulerHandle {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the S04 API and the S08 tray")
    )]
    #[must_use]
    pub fn latest(&self) -> Latest {
        self.latest.borrow().clone()
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the S04 API and the S08 tray")
    )]
    /// A receiver that wakes on every publication.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<Latest> {
        self.latest.clone()
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the S04 API and the S08 tray")
    )]
    /// The snapshot of a scan that started after this call, within [`REFRESH_TIMEOUT`].
    /// Requests are coalesced, not queued per caller, but every one still triggers a scan soon:
    /// an HTTP refresh endpoint (S04) should rate-limit callers.
    pub async fn refresh(&self) -> Result<Arc<Snapshot>, RefreshError> {
        self.refresh_within(REFRESH_TIMEOUT).await
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the S04 API and the S08 tray")
    )]
    pub async fn refresh_within(&self, timeout: Duration) -> Result<Arc<Snapshot>, RefreshError> {
        let (reply, done) = oneshot::channel();
        self.requests
            .send(Request::Refresh(reply))
            .map_err(|_| RefreshError::Stopped)?;
        match tokio::time::timeout(timeout, done).await {
            Ok(Ok(snapshot)) => Ok(snapshot),
            Ok(Err(_)) => Err(RefreshError::Stopped),
            Err(_) => Err(RefreshError::TimedOut),
        }
    }
}

/// Owns the scanner thread. Dropping it asks the thread to stop without waiting.
pub struct Scheduler {
    handle: SchedulerHandle,
    worker: Option<JoinHandle<()>>,
}

impl Scheduler {
    /// Starts the scanner thread; the first scan begins immediately.
    pub fn start(interval: Duration) -> io::Result<Self> {
        Self::start_with(interval, Box::new(scanner_scan))
    }

    fn start_with(interval: Duration, make_scan: MakeScan) -> io::Result<Self> {
        let (requests, inbox) = mpsc::channel();
        let (publisher, latest) = watch::channel(None);
        let worker = thread::Builder::new()
            .name("stackscan-scanner".into())
            .spawn(move || {
                let mut worker = Worker {
                    interval,
                    inbox,
                    publisher,
                    make_scan,
                    scan: None,
                    tracker: SnapshotTracker::new(Some(interval)),
                };
                if panic::catch_unwind(AssertUnwindSafe(|| worker.run())).is_err() {
                    worker.stopped();
                }
            })?;
        Ok(Self {
            handle: SchedulerHandle { requests, latest },
            worker: Some(worker),
        })
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the S04 API and the S08 tray")
    )]
    #[must_use]
    pub fn handle(&self) -> SchedulerHandle {
        self.handle.clone()
    }

    /// Asks the loop to stop after the scan in progress (if any) and waits for the thread up to
    /// `deadline`. Returns `false` if it was still busy: the caller then simply exits, and the
    /// thread ends with the process.
    pub fn shutdown(mut self, deadline: Duration) -> bool {
        let _ = self.handle.requests.send(Request::Shutdown);
        let Some(worker) = self.worker.take() else {
            return true;
        };
        let until = Instant::now() + deadline;
        while !worker.is_finished() {
            if Instant::now() >= until {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = worker.join();
        true
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        let _ = self.handle.requests.send(Request::Shutdown);
    }
}

/// The production scan: one [`Scanner`], built on first use inside the worker's panic guard.
fn scanner_scan() -> Scan {
    let mut scanner = Scanner::new();
    Box::new(move || scanner.scan().map_err(|error| error.to_string()))
}

struct Worker {
    interval: Duration,
    inbox: mpsc::Receiver<Request>,
    publisher: watch::Sender<Latest>,
    make_scan: MakeScan,
    scan: Option<Scan>,
    tracker: SnapshotTracker,
}

impl Worker {
    fn run(&mut self) {
        let mut next_due = Instant::now();
        let mut waiting = Vec::new();
        loop {
            let timeout = next_due.saturating_duration_since(Instant::now());
            match self.inbox.recv_timeout(timeout) {
                Ok(Request::Refresh(reply)) => waiting.push(reply),
                Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            // Coalesce everything already queued into this one scan.
            loop {
                match self.inbox.try_recv() {
                    Ok(Request::Refresh(reply)) => waiting.push(reply),
                    Ok(Request::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return,
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            if waiting.is_empty() && Instant::now() < next_due {
                continue;
            }

            let started = Instant::now();
            let snapshot = Arc::new(self.attempt());
            let finished = Instant::now();
            self.publisher.send_replace(Some(Arc::clone(&snapshot)));
            for reply in waiting.drain(..) {
                let _ = reply.send(Arc::clone(&snapshot));
            }
            next_due = if self.tracker.overran(finished - started) {
                // Back off a full interval rather than scanning back to back.
                finished + self.interval
            } else {
                started + self.interval
            };
        }
    }

    /// One scan. A panic inside it (scanner construction included) is reported like any failure
    /// and the scan is rebuilt, since its retained state may be inconsistent.
    fn attempt(&mut self) -> Snapshot {
        let timer = AttemptTimer::start();
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let scan = self.scan.get_or_insert_with(|| (self.make_scan)());
            scan()
        }));
        let outcome = result.unwrap_or_else(|_| {
            self.scan = None;
            Err("the scanner panicked; it was restarted".to_owned())
        });
        let attempt = timer.finish();
        let previous_failures = self.previous_failures();
        let snapshot = match outcome {
            Ok(observation) => self.tracker.succeeded(observation, attempt),
            Err(failure) => self.tracker.failed(failure, attempt),
        };
        for note in transition_notes(
            &snapshot,
            previous_failures,
            self.tracker.overran(attempt.duration),
        ) {
            report!("{note}");
        }
        snapshot
    }

    /// The loop itself failed: say so in a last snapshot (keeping the last good observation),
    /// then end. Pending refreshes and later ones report [`RefreshError::Stopped`].
    fn stopped(&mut self) {
        let failure = "the scanner stopped unexpectedly; restart Stackscan".to_owned();
        report!("stackscan: {failure}");
        let snapshot = self.tracker.failed(failure, AttemptTimer::start().finish());
        self.publisher.send_replace(Some(Arc::new(snapshot)));
    }

    fn previous_failures(&self) -> u32 {
        self.publisher
            .borrow()
            .as_ref()
            .map_or(0, |snapshot| snapshot.scanner.consecutive_failures)
    }
}

/// Failures, recoveries and overruns are noted once each, not on every tick.
fn transition_notes(snapshot: &Snapshot, previous_failures: u32, overran: bool) -> Vec<String> {
    let status = &snapshot.scanner;
    let mut notes = Vec::new();
    match (&status.failure, previous_failures) {
        (Some(failure), 0) => notes.push(format!(
            "stackscan: scan failed: {failure}; {}",
            if snapshot.observation.is_some() {
                "keeping the last good snapshot"
            } else {
                "no snapshot yet"
            }
        )),
        (None, failures) if failures > 0 => notes.push(format!(
            "stackscan: scanning recovered after {failures} failed attempts"
        )),
        _ => {}
    }
    if overran {
        notes.push(format!(
            "stackscan: scan took {:.0} ms, longer than the {} ms interval ({} overruns so far)",
            status.last_attempt_duration_ms,
            status.interval_ms.unwrap_or_default(),
            status.overruns
        ));
    }
    notes
}

#[cfg(test)]
mod tests {

    use std::{
        collections::BTreeSet,
        net::{IpAddr, Ipv4Addr},
    };

    use stackscan_core::model::CpuUsage;
    use stackscan_fixtures::{Fixture, FixtureSpec};

    use super::*;

    const LONG: Duration = Duration::from_secs(3600);

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    async fn first_snapshot(handle: &SchedulerHandle) -> Arc<Snapshot> {
        let mut changes = handle.subscribe();
        let latest = changes
            .wait_for(Option::is_some)
            .await
            .expect("scheduler running");
        latest.clone().expect("published")
    }

    #[test]
    fn scans_immediately_and_publishes_increasing_revisions() {
        let scheduler = Scheduler::start(LONG).expect("start");
        let handle = scheduler.handle();
        runtime().block_on(async {
            let first = first_snapshot(&handle).await;
            assert_eq!(first.revision, 1);
            assert!(first.observation.is_some());
            assert_eq!(first.scanner.failure, None);
            assert_eq!(first.scanner.interval_ms, Some(3_600_000));
            let refreshed = handle.refresh().await.expect("refresh");
            assert_eq!(refreshed.revision, 2);
            assert_eq!(handle.latest().map(|latest| latest.revision), Some(2));
        });
        assert!(scheduler.shutdown(LONG));
    }

    #[test]
    fn concurrent_refreshes_coalesce_into_scans_that_started_after_them() {
        let scheduler = Scheduler::start(LONG).expect("start");
        let handle = scheduler.handle();
        runtime().block_on(async {
            first_snapshot(&handle).await;
            let requested_at = stackscan_core::scan::unix_millis(std::time::SystemTime::now());
            // Spawned together, so every request is queued before any reply is awaited.
            let tasks: Vec<_> = (0..20)
                .map(|_| {
                    let handle = handle.clone();
                    tokio::spawn(async move { handle.refresh().await })
                })
                .collect();
            let mut replies = Vec::with_capacity(tasks.len());
            for task in tasks {
                replies.push(task.await.expect("task").expect("refresh"));
            }
            let revisions: BTreeSet<u64> =
                replies.iter().map(|snapshot| snapshot.revision).collect();
            assert!(
                revisions.len() <= 2,
                "20 requests, {} scans: {revisions:?}",
                revisions.len()
            );
            for snapshot in &replies {
                let observed = snapshot.observation.as_ref().expect("observation");
                assert!(
                    observed.scanned_at_ms >= requested_at,
                    "scan started after the request"
                );
            }
        });
        assert!(scheduler.shutdown(LONG));
    }

    #[test]
    fn ticks_sample_cpu_of_a_busy_fixture() {
        let busy = Fixture::spawn(
            &FixtureSpec::new()
                .listen(&[IpAddr::V4(Ipv4Addr::LOCALHOST)])
                .busy(),
        )
        .expect("busy fixture");
        let scheduler = Scheduler::start(Duration::from_millis(500)).expect("start");
        let handle = scheduler.handle();
        let cpu_at = |snapshot: &Snapshot| {
            snapshot
                .observation
                .as_ref()
                .and_then(|observation| {
                    observation
                        .services
                        .iter()
                        .find(|service| service.process.identity.pid == busy.pid())
                })
                .map_or_else(
                    || panic!("fixture {} missing", busy.pid()),
                    |service| service.process.metrics.cpu,
                )
        };
        runtime().block_on(async {
            let first = first_snapshot(&handle).await;
            assert_eq!(cpu_at(&first), CpuUsage::WarmingUp);
            let mut changes = handle.subscribe();
            let later = changes
                .wait_for(|latest| {
                    latest
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.revision > 1)
                })
                .await
                .expect("second tick")
                .clone()
                .expect("published");
            let CpuUsage::Sampled { percent } = cpu_at(&later) else {
                panic!("second tick must sample: {:?}", cpu_at(&later));
            };
            assert!(percent > 40.0, "busy fixture at {percent} %");
        });
        assert!(scheduler.shutdown(LONG));
    }

    #[test]
    fn shutdown_stops_the_thread_and_refresh_reports_it() {
        let scheduler = Scheduler::start(LONG).expect("start");
        let handle = scheduler.handle();
        assert!(scheduler.shutdown(LONG));
        runtime().block_on(async {
            assert_eq!(handle.refresh().await.err(), Some(RefreshError::Stopped));
        });
    }

    /// One scripted attempt. The scheduler's failure handling is a state machine over attempt
    /// outcomes, so these inject outcomes instead of breaking the real scanner.
    enum Step {
        Succeed,
        Fail(&'static str),
        Panic,
        Slow(Duration),
    }

    struct Script {
        steps: std::sync::Mutex<std::collections::VecDeque<Step>>,
        builds: std::sync::atomic::AtomicUsize,
        panic_on_first_build: bool,
    }

    /// Scans follow `steps`, then succeed. Returns the script to inspect builds afterwards.
    fn scripted(steps: Vec<Step>, panic_on_first_build: bool) -> (Arc<Script>, MakeScan) {
        let script = Arc::new(Script {
            steps: std::sync::Mutex::new(steps.into()),
            builds: std::sync::atomic::AtomicUsize::new(0),
            panic_on_first_build,
        });
        let shared = Arc::clone(&script);
        let make: MakeScan = Box::new(move || {
            let builds = shared
                .builds
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(
                !(shared.panic_on_first_build && builds == 0),
                "scanner construction panicked"
            );
            let shared = Arc::clone(&shared);
            Box::new(move || {
                let step = shared.steps.lock().expect("script").pop_front();
                match step {
                    None | Some(Step::Succeed) => Ok(empty_observation()),
                    Some(Step::Fail(reason)) => Err(reason.to_owned()),
                    Some(Step::Panic) => panic!("injected scan panic"),
                    Some(Step::Slow(duration)) => {
                        thread::sleep(duration);
                        Ok(empty_observation())
                    }
                }
            })
        });
        (script, make)
    }

    fn empty_observation() -> Observation {
        Observation {
            scanned_at_ms: stackscan_core::scan::unix_millis(std::time::SystemTime::now()),
            scan_duration_ms: 0.1,
            services: Vec::new(),
            projects: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn failures_keep_the_last_good_observation_until_recovery() {
        let (_, make) = scripted(
            vec![
                Step::Succeed,
                Step::Fail("enumeration failed"),
                Step::Fail("enumeration failed"),
            ],
            false,
        );
        let scheduler = Scheduler::start_with(LONG, make).expect("start");
        let handle = scheduler.handle();
        runtime().block_on(async {
            let good = first_snapshot(&handle).await;
            assert_eq!(good.scanner.failure, None);
            let first_failure = handle.refresh().await.expect("refresh");
            let second_failure = handle.refresh().await.expect("refresh");
            for failed in [&first_failure, &second_failure] {
                assert_eq!(failed.observation, good.observation, "last good kept");
                assert_eq!(
                    failed.scanner.failure.as_deref(),
                    Some("enumeration failed")
                );
                assert_eq!(failed.instance, good.instance);
            }
            assert_eq!(second_failure.scanner.consecutive_failures, 2);
            assert!(second_failure.scanner.last_attempt_at_ms >= good.scanner.last_attempt_at_ms);
            let recovered = handle.refresh().await.expect("refresh");
            assert_eq!(recovered.scanner.failure, None);
            assert_eq!(recovered.scanner.consecutive_failures, 0);
            assert_eq!(recovered.revision, 4);
        });
        assert!(scheduler.shutdown(LONG));
    }

    #[test]
    fn panics_in_scans_and_in_scanner_construction_are_contained() {
        let (script, make) = scripted(vec![Step::Panic], true);
        let scheduler = Scheduler::start_with(LONG, make).expect("start");
        let handle = scheduler.handle();
        runtime().block_on(async {
            // First build panics, the rebuilt scan panics once, then scans succeed.
            let first = first_snapshot(&handle).await;
            assert!(
                first
                    .scanner
                    .failure
                    .as_deref()
                    .is_some_and(|f| f.contains("panicked"))
            );
            assert_eq!(
                first.observation, None,
                "no observation rather than an empty one"
            );
            let second = handle.refresh().await.expect("scheduler still running");
            assert!(second.scanner.failure.is_some());
            let third = handle.refresh().await.expect("scheduler still running");
            assert_eq!(third.scanner.failure, None);
            assert!(third.observation.is_some());
        });
        assert_eq!(
            script.builds.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "rebuilt after each panic"
        );
        assert!(scheduler.shutdown(LONG));
    }

    #[test]
    fn an_overrun_is_counted_and_followed_by_a_full_interval() {
        let interval = Duration::from_millis(100);
        let (_, make) = scripted(vec![Step::Slow(Duration::from_millis(150))], false);
        let scheduler = Scheduler::start_with(interval, make).expect("start");
        let handle = scheduler.handle();
        runtime().block_on(async {
            let slow = first_snapshot(&handle).await;
            assert_eq!(slow.scanner.overruns, 1);
            let mut changes = handle.subscribe();
            let next = changes
                .wait_for(|latest| {
                    latest
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.revision > 1)
                })
                .await
                .expect("next tick")
                .clone()
                .expect("published");
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a 150 ms duration"
            )]
            let slow_end =
                slow.scanner.last_attempt_at_ms + slow.scanner.last_attempt_duration_ms as u64;
            // Millisecond wall-clock rounding on both ends.
            assert!(
                next.scanner.last_attempt_at_ms + 2 >= slow_end + 100,
                "next scan started {} ms after the overrun ended",
                next.scanner.last_attempt_at_ms.saturating_sub(slow_end)
            );
            assert_eq!(next.scanner.overruns, 1);
        });
        assert!(scheduler.shutdown(LONG));
    }

    #[test]
    fn refresh_and_shutdown_return_within_their_deadlines_during_a_stuck_scan() {
        let (_, make) = scripted(
            vec![Step::Succeed, Step::Slow(Duration::from_secs(1))],
            false,
        );
        let scheduler = Scheduler::start_with(LONG, make).expect("start");
        let handle = scheduler.handle();
        runtime().block_on(async {
            first_snapshot(&handle).await;
            let started = Instant::now();
            let result = handle.refresh_within(Duration::from_millis(100)).await;
            assert_eq!(result.err(), Some(RefreshError::TimedOut));
            assert!(started.elapsed() < Duration::from_millis(500));
        });
        let started = Instant::now();
        assert!(
            !scheduler.shutdown(Duration::from_millis(50)),
            "still scanning"
        );
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn transitions_are_noted_once() {
        let mut tracker = SnapshotTracker::new(Some(Duration::from_secs(3)));
        let attempt = |millis| stackscan_core::snapshot::Attempt {
            started_at_ms: 1,
            duration: Duration::from_millis(millis),
        };
        let failed = tracker.failed("boom".into(), attempt(1));
        assert_eq!(transition_notes(&failed, 0, false).len(), 1);
        let still_failing = tracker.failed("boom".into(), attempt(1));
        assert_eq!(
            transition_notes(&still_failing, 1, false),
            Vec::<String>::new()
        );
        let recovered = tracker.succeeded(empty_observation(), attempt(1));
        assert_eq!(
            transition_notes(&recovered, 2, false),
            ["stackscan: scanning recovered after 2 failed attempts"]
        );
        let steady = tracker.succeeded(empty_observation(), attempt(1));
        assert_eq!(transition_notes(&steady, 0, false), Vec::<String>::new());
        let slow = tracker.succeeded(empty_observation(), attempt(4_000));
        let notes = transition_notes(&slow, 0, true);
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0].contains("longer than the 3000 ms interval"),
            "{notes:?}"
        );
    }
}
