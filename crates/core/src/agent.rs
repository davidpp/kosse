//! The JSON records of the agent-facing commands (`kosse url`, `kosse port` and their errors; see
//! blueprint/agent-surface.md; `kosse profile`'s are in [`crate::profiles::authoring`]). `kosse list --json` prints the [`crate::model::Snapshot`] itself.
//! Wire contracts, generated into TypeScript like the rest of the model.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{ProcessIdentity, Project, Service};

/// Bumped when a change to [`UrlAnswer`], [`PortAnswer`] or a `kosse profile` record
/// ([`crate::profiles::authoring`]) would break an existing reader.
pub const AGENT_SCHEMA_VERSION: u32 = 1;

/// `kosse url --json`: the one endpoint of the named service (or port) that answered HTTP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UrlAnswer {
    pub schema_version: u32,
    /// `http://localhost:<port>/`, or `https://` when the answer came over verified TLS.
    pub url: String,
    pub port: u16,
    pub identity: ProcessIdentity,
    /// `Project.root` of the service; `null` outside a project.
    pub project: Option<String>,
    /// The configured service ID it matched; `null` when it has none.
    pub service: Option<String>,
}

/// `kosse port <n> --json`: who listens on a TCP port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PortAnswer {
    pub schema_version: u32,
    pub port: u16,
    pub state: PortState,
    /// `false` when other users' listeners (system services included) are hidden from this user,
    /// so `free` means "free as far as this user can see", never a definite free.
    pub other_users_visible: bool,
    /// When the observation answering this was taken, in Unix milliseconds.
    #[ts(type = "number")]
    pub scanned_at_ms: u64,
    /// The services listening on the port; empty when `free`.
    pub services: Vec<Service>,
    /// The projects those services run in.
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PortState {
    Free,
    Taken,
}

/// What an agent-facing command prints on stderr with `--json` when it has no answer, shaped
/// like the API's `ApiError`. The exit code carries the class (1 negative answer, 2 usage, 3
/// refused, 4 could not observe); `error` says which case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub error: CommandErrorCode,
    /// A human-readable explanation; not meant for parsing.
    pub message: String,
    /// For `invalidProfile`: where the profile breaks a rule. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<ProfileFieldProblem>>", optional)]
    pub problems: Vec<ProfileFieldProblem>,
}

/// One rule a refused profile breaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFieldProblem {
    /// A JSON-path-like location such as `services.web.match.cwd`; empty for the whole file.
    pub at: String,
    pub problem: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CommandErrorCode {
    /// No observed service matches the service name or port (exit 1).
    NotFound,
    /// Several services or endpoints match; narrow with `--here`, `--project` or a port (exit 1).
    Ambiguous,
    /// The service listens, but no endpoint of it answered HTTP; `message` gives the probe state
    /// (exit 1).
    NoHttp,
    /// The directory (`--here`, `--project`, a `kosse profile` root) is in no project (exit 2).
    NotInProject,
    /// `kosse profile set --file` or `check` could not read its input (exit 2).
    InputUnreadable,
    /// The profile breaks a rule; `problems` says where. Nothing was written (exit 3).
    InvalidProfile,
    /// The stored profile's revision is not the `--expect`ed one: another writer changed it.
    /// Nothing was written (exit 3).
    RevisionConflict,
    /// The root holds control or invisible formatting characters, so no prompt is made (exit 3).
    UnsafeRoot,
    /// The directory's project could not be determined (unreadable, `git` failed) (exit 4).
    ProjectUnknown,
    /// Listening sockets could not be enumerated (exit 4).
    ScanFailed,
    /// Kossé's configuration cannot be read, is invalid or is from a newer Kossé (exit 4).
    ConfigUnavailable,
    /// Writing Kossé's configuration failed (permissions, disk); nothing was changed (exit 4).
    ConfigWriteFailed,
    /// The answer could not be written to stdout (exit 4).
    OutputFailed,
}
