//! Stackscan's observational core: TCP listener discovery ([`sockets`]), process inspection
//! ([`inspect`]), project attribution ([`attribution`]), CPU sampling ([`metrics`]), scans into
//! an [`model::Observation`] ([`scan`]), published [`model::Snapshot`]s ([`snapshot`]), and the
//! wire contracts generated into TypeScript ([`contract`]). Nothing here depends on a UI, router
//! or async runtime.

pub mod attribution;
pub mod contract;
mod endpoints;
mod git;
pub mod inspect;
pub mod metrics;
pub mod model;
pub mod scan;
pub mod snapshot;
pub mod sockets;
