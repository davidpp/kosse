//! What the snapshot says about project metadata: effective values with their provenance, which
//! processes each configured service matches, and why a profile source is not in use. Wire
//! contracts, generated into TypeScript like the rest of the model.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    model::ProcessIdentity,
    profiles::document::{DeclaredProtocol, EndpointShow, LinkKind, ServiceMatch},
};

/// Where an effective value came from. Precedence: central over repository over inference; a
/// field without a source is not configured and falls back to what was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProfileSource {
    /// Kossé's per-user configuration ("Customize this project").
    Central,
    /// The project's own `kosse.json`, read only.
    Repository,
}

/// The effective metadata of one project root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    /// Canonical worktree root: the key of a central profile, the directory of a repository file.
    pub root: String,
    /// The project is in this observation. Otherwise it is known only from a saved central
    /// profile (or a repository file seen within the last minute): configured, not observed.
    pub observed: bool,
    /// Sources in effect, in precedence order, each once.
    pub sources: Vec<ProfileSource>,
    /// Display name; `null` keeps the observed directory name.
    pub name: Option<String>,
    pub name_source: Option<ProfileSource>,
    /// The stored central profile's revision (`"none"` without one): what a change made from this
    /// metadata sends as `expectedRevision`, and how a view notices another writer's change.
    pub central_revision: String,
    /// Ordered by ID.
    pub services: Vec<ConfiguredService>,
}

/// One configured service after merging its central and repository entries field by field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguredService {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "match")]
    #[ts(rename = "match")]
    pub matcher: Option<ServiceMatch>,
    pub protocol: Option<DeclaredProtocol>,
    pub probe_path: Option<String>,
    /// Listed endpoints, ordered by port; empty when none are configured. Each observed
    /// endpoint's resolved display is on `Endpoint.display`. (Defaults keep an older instance's
    /// snapshot of the same schema version readable.)
    #[serde(default)]
    pub endpoints: Vec<ConfiguredEndpoint>,
    /// How endpoints `endpoints` does not list are shown; `null` when not configured.
    #[serde(default)]
    pub other_endpoints: Option<EndpointShow>,
    /// Empty when none are configured.
    pub links: Vec<EffectiveLink>,
    /// Suppressed: it matches nothing and only shows that an entry was turned off.
    pub disabled: bool,
    pub sources: ServiceSources,
    /// Processes this entry matches in the current observation, ordered by PID; each remains its
    /// own action target. Empty means configured, not observed: neither running nor stopped. A
    /// process matched by several entries is listed by each and is ambiguous on its service.
    pub matched: Vec<ProcessIdentity>,
}

/// Which source set each field of a [`ConfiguredService`]; `null` when no source set it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSources {
    pub name: Option<ProfileSource>,
    #[serde(rename = "match")]
    #[ts(rename = "match")]
    pub matcher: Option<ProfileSource>,
    pub protocol: Option<ProfileSource>,
    pub probe_path: Option<ProfileSource>,
    #[serde(default)]
    pub endpoints: Option<ProfileSource>,
    #[serde(default)]
    pub other_endpoints: Option<ProfileSource>,
    pub links: Option<ProfileSource>,
    pub disabled: Option<ProfileSource>,
}

/// One endpoint as a profile lists it: unset fields stay `null` (a listed endpoint without
/// `show` is shown in the summary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguredEndpoint {
    pub port: u16,
    pub name: Option<String>,
    pub show: Option<EndpointShow>,
    pub probe_path: Option<String>,
}

/// A configured link, validated: `http(s)`, no credentials, host shown separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveLink {
    pub label: String,
    /// Scheme and host lowercased. Open only on user action.
    pub url: String,
    /// Shown next to the label so the destination is always visible.
    pub host: String,
    /// The profile's own label for the link (or the default from `reach`). Display only: a
    /// repository can call an external host `local`, so trust comes from `reach` and the visible
    /// `host`, never from `kind`.
    pub kind: LinkKind,
    pub reach: LinkReach,
}

/// What a link's host is, judged from its text alone (nothing is resolved or contacted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LinkReach {
    /// `localhost`, a `.localhost` name, `127.0.0.0/8` or `::1`: may be checked against this
    /// machine's observed loopback endpoints.
    Loopback,
    /// Any other host (custom domains such as `.local` included): declared, never probed or
    /// assumed to be local.
    Declared,
}

/// Which configured service a process is, on its `Service`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProfileMatch {
    Matched {
        service: String,
    },
    /// Several entries match: none is applied, rather than an arbitrary first one.
    Ambiguous {
        services: Vec<String>,
    },
}

/// Why a profile source is not (or no longer) in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProfileProblem {
    /// The file exists but could not be read.
    Unreadable {
        reason: String,
    },
    /// A symbolic link (repository files), directory or device: not read.
    NotAFile,
    TooLarge {
        limit_bytes: u32,
    },
    /// Not valid JSON, or a rule is broken. Untrusted text: it may quote the file.
    Invalid {
        reason: String,
    },
    /// Written for a newer Kossé (`version` as found). Never overwritten.
    NewerVersion {
        version: String,
    },
}

impl fmt::Display for ProfileSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Central => "central",
            Self::Repository => "repository",
        })
    }
}

impl fmt::Display for ProfileProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { reason } => write!(f, "unreadable ({reason})"),
            Self::NotAFile => f.write_str("not a regular file"),
            Self::TooLarge { limit_bytes } => write!(f, "larger than {limit_bytes} bytes"),
            Self::Invalid { reason } => write!(f, "invalid: {reason}"),
            Self::NewerVersion { version } => {
                write!(f, "written for a newer Kossé (version {version})")
            }
        }
    }
}
