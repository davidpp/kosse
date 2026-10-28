//! Stackscan's observational core: TCP listener discovery ([`sockets`]), process inspection
//! ([`inspect`]), one-shot scans into a [`model::Snapshot`] ([`scan`]), and the wire contracts
//! generated into TypeScript ([`contract`]). Nothing here depends on a UI, router or runtime.

pub mod contract;
mod endpoints;
pub mod inspect;
pub mod model;
pub mod scan;
pub mod sockets;
