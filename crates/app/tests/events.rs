//! The snapshot event stream through the real binary: revisions, reconnects, restarts, and
//! shutdown with a client still subscribed.

mod support;

use std::time::{Duration, Instant};

use support::{EventStream, Server};

/// One scan interval (3 s) plus slack.
const NEXT_TICK: Duration = Duration::from_secs(5);

#[test]
fn stream_starts_with_the_current_snapshot_and_follows_revisions() {
    let server = Server::start();
    let mut events = EventStream::open(&server);
    assert_eq!(events.status, 200);
    assert!(
        events.headers.contains("content-type: text/event-stream"),
        "{}",
        events.headers
    );

    let first = events.next_snapshot(NEXT_TICK).expect("current snapshot");
    let current = server.snapshot();
    assert_eq!(first.instance, current.instance);
    assert!(first.revision <= current.revision);
    let next = events.next_snapshot(NEXT_TICK).expect("next tick");
    assert_eq!(next.instance, first.instance);
    assert!(next.revision > first.revision);

    // A refresh is pushed without waiting for the tick.
    let token = server.token();
    let started = Instant::now();
    let refreshed = server.post_json("/api/refresh", &token, "{}");
    assert_eq!(refreshed.status, 200);
    let pushed = events.next_snapshot(NEXT_TICK).expect("refresh pushed");
    assert!(pushed.revision > next.revision);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn reconnecting_resumes_from_a_fresh_snapshot_and_a_restart_is_a_new_instance() {
    let server = Server::start();
    let mut events = EventStream::open(&server);
    let seen = events.next_snapshot(NEXT_TICK).expect("snapshot");
    drop(events);

    // Missed publications are not replayed: the first event after reconnecting is the newest.
    std::thread::sleep(Duration::from_millis(3500));
    let mut events = EventStream::open(&server);
    let resumed = events
        .next_snapshot(NEXT_TICK)
        .expect("snapshot after reconnect");
    assert_eq!(resumed.instance, seen.instance);
    assert!(
        resumed.revision > seen.revision,
        "{} then {}",
        seen.revision,
        resumed.revision
    );

    let restarted = Server::start();
    let mut events = EventStream::open(&restarted);
    let other = events.next_snapshot(NEXT_TICK).expect("snapshot");
    assert_ne!(
        other.instance, seen.instance,
        "a new process is a new instance"
    );
}

#[test]
fn open_event_streams_do_not_hold_shutdown() {
    let server = Server::start();
    let mut first = EventStream::open(&server);
    let mut second = EventStream::open(&server);
    assert!(first.next_snapshot(NEXT_TICK).is_some());
    assert!(second.next_snapshot(NEXT_TICK).is_some());

    let started = Instant::now();
    let (status, stderr) = server.terminate();
    let took = started.elapsed();

    assert!(status.success(), "exit status {status}: {stderr}");
    assert!(took < Duration::from_secs(2), "took {took:?}");
    assert!(
        !stderr.contains("dropping connections"),
        "streams ended cleanly: {stderr}"
    );
    assert!(stderr.contains("stackscan stopped"), "{stderr}");
    assert!(
        first.next_snapshot(Duration::from_millis(500)).is_none(),
        "stream ended"
    );
}
