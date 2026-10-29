//! Types serialized to the browser. TypeScript declarations are generated from these definitions,
//! so the web client never hand-maintains a copy of the model. The service model itself is in
//! [`crate::model`]; this module adds the API's own request and response types.

use std::{any::TypeId, collections::HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS, TypeVisitor};

/// Identifies the Kossé instance answering on this endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub name: String,
    pub version: String,
    pub mode: InstanceMode,
}

/// How the instance runs, so a second launch knows what it can ask of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstanceMode {
    /// The menu-bar app (`kosse`): it owns a dashboard window and takes `POST /api/window`.
    Resident,
    /// `kosse open` serving only for its own window's lifetime.
    Ephemeral,
    /// `kosse serve`: no tray, no window.
    Headless,
}

/// `POST /api/window`: show the resident app's dashboard window at `route` and bring it forward.
/// Only a [`InstanceMode::Resident`] instance accepts it (`409 notResident` otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowRequest {
    pub route: WindowRoute,
}

/// A dashboard address; the identity routes name a process by PID and start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WindowRoute {
    Dashboard,
    Inspect { pid: u32, started_at_secs: u64 },
    Stop { pid: u32, started_at_secs: u64 },
}

/// The per-instance action token. Unsafe API requests carry it in the `X-Kosse-Token`
/// header. It is served only to same-origin requests and changes on every restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token: String,
}

/// `POST /api/probe`: probe one endpoint now ("try HTTP"). The identity must match the current
/// snapshot, and the endpoint must allow requests (see `ProbeSkip`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeRequest {
    pub identity: crate::model::ProcessIdentity,
    pub port: u16,
}

/// `POST /api/profile` ("Customize this project"): saves (`profile` set) or removes (`null`) the
/// central profile of `root`, which must be a project in the current snapshot or one that already
/// has a central profile. Only Kossé's own configuration is written, never the project. Fields
/// the dashboard does not know, and `$schema`, are kept from the profile being replaced: per
/// service ID, and per link by URL (a link whose URL changes loses them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveProfileRequest {
    pub root: String,
    pub profile: Option<crate::profiles::ProjectProfile>,
}

/// The central profile of `root` as stored; the next snapshot applies it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedProfile {
    pub root: String,
    pub profile: Option<crate::profiles::ProjectProfile>,
}

/// The dashboard's preferences, kept in Kossé's configuration file (never in a project).
/// `GET /api/preferences` returns them (defaults when there is no file yet); `POST
/// /api/preferences` replaces them and returns them as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    /// Show listeners outside a project and other users' processes ("All listeners").
    pub all_listeners: bool,
}

/// The body of every API error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub error: ApiErrorCode,
    /// A human-readable explanation; not meant for parsing.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ApiErrorCode {
    /// The `Host` (or request-target authority) is not this instance's loopback authority.
    HostNotAllowed,
    /// A cross-origin request, or an unsafe one without this instance's exact `Origin`.
    OriginNotAllowed,
    /// An unsafe request without the current action token (see [`Session`]).
    TokenRequired,
    /// Unsafe requests must send `Content-Type: application/json`.
    UnsupportedMediaType,
    /// The body exceeds 16 KiB: refused before reading when `Content-Length` says so, and while
    /// reading for a chunked body.
    PayloadTooLarge,
    /// The JSON body does not match the request type.
    InvalidPayload,
    /// Too many refreshes or requested probes; retry after the `Retry-After` seconds.
    RateLimited,
    /// The process identity (PID + start time) or port is not in the current snapshot: the
    /// process ended or the PID was reused. Refresh and retry.
    StaleIdentity,
    /// This endpoint may not be probed: another user's process, or not on loopback.
    NotProbeable,
    /// No scan finished in time (the scanner is busy or stalled); the last snapshot still
    /// stands.
    ScannerBusy,
    /// The scanner stopped; restart Kossé.
    ScannerStopped,
    /// The profile breaks a rule; `message` says which field and why.
    InvalidProfile,
    /// The root is neither a project in the current snapshot nor one with a saved profile.
    UnknownProject,
    /// The configuration file cannot be read, is invalid or is from a newer Kossé, so it is not
    /// overwritten; `message` names it. Fix or remove it.
    ConfigUnavailable,
    /// Writing the configuration failed (permissions, disk); `message` says why. Nothing was
    /// changed.
    ConfigWriteFailed,
    /// Only the menu-bar app has a window to show (`POST /api/window`).
    NotResident,
}

/// Every contract type in one TypeScript module.
#[must_use]
pub fn typescript_bindings() -> String {
    let mut module = BindingsModule::new();
    // Roots: the types API handlers serialize. Types they reference are included automatically.
    module.add::<Health>();
    module.add::<Session>();
    module.add::<ApiError>();
    module.add::<ProbeRequest>();
    module.add::<crate::action::ActionRequest>();
    module.add::<crate::action::ActionResult>();
    module.add::<SaveProfileRequest>();
    module.add::<SavedProfile>();
    module.add::<Preferences>();
    module.add::<WindowRequest>();
    module.add::<crate::model::Snapshot>();
    module.add::<crate::agent::UrlAnswer>();
    module.add::<crate::agent::PortAnswer>();
    module.add::<crate::agent::CommandError>();
    module.finish()
}

const BINDINGS_HEADER: &str = "// Generated from the Rust contract types in crates/core. Do not edit.\n\
// Regenerate with `bun run contracts`.\n";

/// Collects declarations dependency-first, each exactly once, so the module needs no imports.
struct BindingsModule {
    cfg: Config,
    declared: HashSet<TypeId>,
    source: String,
}

impl BindingsModule {
    fn new() -> Self {
        Self {
            cfg: Config::new(),
            declared: HashSet::new(),
            source: String::from(BINDINGS_HEADER),
        }
    }

    fn add<T: TS + 'static + ?Sized>(&mut self) {
        // Only derived types have an output path; std containers and primitives are inlined,
        // but whatever they wrap (e.g. `Vec<Endpoint>`) still needs declaring.
        if T::output_path().is_none() {
            self.add_dependencies_of::<T>();
            return;
        }
        if !self.declared.insert(TypeId::of::<T>()) {
            return;
        }
        self.add_dependencies_of::<T>();
        self.source.push('\n');
        if let Some(docs) = T::docs() {
            self.source.push_str(&docs);
        }
        self.source.push_str("export ");
        self.source.push_str(&T::decl(&self.cfg));
        self.source.push('\n');
    }

    /// The derive macro enumerates dependencies from a `HashSet`, so their order changes between
    /// builds; visiting them by name keeps the generated module byte-stable.
    fn add_dependencies_of<T: TS + 'static + ?Sized>(&mut self) {
        let mut dependencies = Dependencies {
            cfg: &self.cfg,
            found: Vec::new(),
        };
        T::visit_dependencies(&mut dependencies);
        let mut found = dependencies.found;
        found.sort_by(|left, right| left.0.cmp(&right.0));
        for (_, add) in found {
            add(self);
        }
    }

    fn finish(self) -> String {
        self.source
    }
}

/// Declares one type (and its dependencies) in the module.
type Declare = fn(&mut BindingsModule);

/// A type's direct dependencies, by TypeScript name.
struct Dependencies<'cfg> {
    cfg: &'cfg Config,
    found: Vec<(String, Declare)>,
}

impl TypeVisitor for Dependencies<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        self.found
            .push((T::name(self.cfg), BindingsModule::add::<T>));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Leaf {
        value: u32,
    }

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Branch {
        first: Leaf,
        rest: Vec<Leaf>,
        maybe: Option<Leaf>,
    }

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Zeta {
        value: u32,
    }

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Alpha {
        value: u32,
    }

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Mixed {
        zeta: Zeta,
        leaf: Leaf,
        alpha: Vec<Alpha>,
    }

    #[test]
    fn referenced_types_are_declared_once_before_their_users() {
        let mut module = BindingsModule::new();
        module.add::<Branch>();
        let source = module.finish();

        assert_eq!(source.matches("export type Leaf =").count(), 1, "{source}");
        let leaf = source.find("export type Leaf =").expect("Leaf declared");
        let branch = source
            .find("export type Branch =")
            .expect("Branch declared");
        assert!(leaf < branch, "{source}");
    }

    #[test]
    fn sibling_dependencies_are_declared_in_name_order() {
        let mut module = BindingsModule::new();
        module.add::<Mixed>();
        let source = module.finish();

        let position = |name: &str| {
            source
                .find(&format!("export type {name} ="))
                .unwrap_or_else(|| panic!("{name} declared in {source}"))
        };
        assert!(position("Alpha") < position("Leaf"), "{source}");
        assert!(position("Leaf") < position("Zeta"), "{source}");
        assert!(position("Zeta") < position("Mixed"), "{source}");
    }
}
