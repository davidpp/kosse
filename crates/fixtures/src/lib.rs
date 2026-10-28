//! Test infrastructure for real processes: owned fixture listeners with an identity manifest, and
//! `lsof` as independent ground truth. Used only as a dev-dependency; nothing here ships.

mod fixture;
pub mod lsof;
pub mod protocol;

pub use fixture::{Cleanup, Fixture, FixtureError, FixtureRecord, FixtureSpec, live_start_time};
