//! Kossé's observational core: TCP listener discovery ([`sockets`]), process inspection
//! ([`inspect`]), project attribution ([`attribution`]), CPU sampling ([`metrics`]), scans into
//! an [`model::Observation`] ([`scan`]), published [`model::Snapshot`]s ([`snapshot`]), central
//! secret redaction ([`redact`]), the HTTP probe policy ([`probe`]), the stop policy
//! ([`action`]) and its one signal call ([`signal`]), optional project metadata ([`profiles`]),
//! and the wire contracts generated into TypeScript ([`contract`]). Nothing here depends on a UI,
//! router or async
//! runtime.

pub mod action;
pub mod attribution;
pub mod contract;
mod endpoints;
mod git;
pub mod inspect;
pub mod metrics;
pub mod model;
pub mod probe;
pub mod profiles;
pub mod redact;
pub mod scan;
pub mod signal;
pub mod snapshot;
pub mod sockets;
