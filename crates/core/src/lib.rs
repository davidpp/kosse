//! Kossé's observational core: TCP listener discovery ([`sockets`]), process inspection
//! ([`inspect`]), project attribution ([`attribution`]), CPU sampling ([`metrics`]), scans into
//! an [`model::Observation`] ([`scan`]), published [`model::Snapshot`]s ([`snapshot`]), central
//! secret redaction ([`redact`]), the HTTP probe policy ([`probe`]), the stop policy
//! ([`action`]) and its one signal call ([`signal`]), optional project metadata ([`profiles`]),
//! the wire contracts generated into TypeScript ([`contract`]) and the agent commands' records
//! ([`agent`]). Nothing here depends on a UI, router or async runtime.

// The workspace denies `unsafe_code` (the app opts in at its AppKit/WebKit call sites); the core
// has none and may never gain any.
#![forbid(unsafe_code)]

pub mod action;
pub mod agent;
pub mod attribution;
pub mod contract;
mod endpoints;
pub mod git;
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
