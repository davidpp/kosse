//! Optional project metadata (blueprint/project-metadata.md): central profiles keyed by canonical
//! worktree root ([`central`]), an optional read-only `kosse.json` at a detected project root,
//! merged central over repository over inference with provenance ([`effective`]), and applied to
//! each observation by the scanner ([`watch::Profiles`]). Agents author central profiles from
//! [`authoring`]'s facts and preview, starting from [`prompt`].
//!
//! Metadata is passive data. Reading it never runs commands, expands variables, follows
//! includes, fetches a `$schema`, resolves or contacts a link's host, or changes system or network
//! configuration. Writing it only ever touches the central file: never a monitored repository.

pub mod authoring;
pub mod central;
pub mod document;
pub mod effective;
pub mod prompt;
pub mod source;
pub mod validate;
pub mod view;
pub mod watch;

pub use central::{CentralConfig, CentralStore, StoreError};
pub use document::{ProjectProfile, json_schema};
pub use effective::probe_path;
pub use watch::Profiles;
