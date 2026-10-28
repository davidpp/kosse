//! `inspect::vitals` against an owned fixture: a killed but unreaped child (a zombie, which
//! sysinfo drops) reads as exited, never as unknown or running.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

use kosse_core::inspect::{Presence, vitals};
use kosse_fixtures::{Fixture, FixtureSpec};

#[test]
fn a_zombie_reads_as_exited() {
    let mut fixture =
        Fixture::spawn(&FixtureSpec::new().listen(&[IpAddr::V4(Ipv4Addr::LOCALHOST)]))
            .expect("fixture");
    assert!(matches!(vitals(fixture.pid()), Presence::Running(_)));
    // SIGKILL through the fixture's own handle, without reaping it.
    fixture.kill().expect("kill");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut presence = vitals(fixture.pid());
    while presence != Presence::Exited && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        presence = vitals(fixture.pid());
    }
    assert_eq!(presence, Presence::Exited);
    fixture.wait(Duration::from_secs(5)).expect("reap");
}
