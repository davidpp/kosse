//! Test infrastructure for real processes: owned fixture listeners with an identity manifest,
//! `lsof` as independent ground truth, and disposable Git repositories. Used only as a
//! dev-dependency; nothing here ships.

mod fixture;
pub mod lsof;
pub mod protocol;
pub mod repo;

pub use fixture::{Cleanup, Fixture, FixtureError, FixtureRecord, FixtureSpec, live_start_time};
