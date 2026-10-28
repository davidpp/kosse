//! The observed service model shared by every view. These types are wire contracts: TypeScript
//! declarations are generated from them (see [`crate::contract`]).

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped when a change to [`Snapshot`] would break an existing reader. Version 2 moved the
/// observed data into [`Snapshot::observation`] and added the scanner status, attribution and
/// metrics.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

/// What one Kossé instance currently knows: the last successful observation plus how the
/// scanner has fared since. Views never scan; they read the latest published snapshot.
///
/// Staleness: the observation is current when `scanner.failure` is `null` and it is younger than
/// about two `scanner.intervalMs`. A failing scanner keeps publishing the last good observation
/// with `failure` set; a scanner that hangs publishes nothing, so readers judge age from
/// `observation.scannedAtMs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    /// Opaque ID of the publishing Kossé process; it changes on every restart. Order
    /// snapshots by `revision` only within one instance: a new `instance` is a restart (revisions
    /// start again at 1), not an out-of-order event.
    pub instance: String,
    /// Increases with every publication by this instance (successful or failed attempt), so
    /// readers can order snapshots and drop out-of-order ones. A one-shot scan is revision 1.
    #[ts(type = "number")]
    pub revision: u64,
    /// The last successful observation; `null` only when no scan has succeeded yet.
    pub observation: Option<Observation>,
    pub scanner: ScannerStatus,
}

/// One successful scan of the machine's TCP listeners, the processes that own them and the
/// projects they run in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// Wall-clock time the scan started, in Unix milliseconds.
    #[ts(type = "number")]
    pub scanned_at_ms: u64,
    pub scan_duration_ms: f64,
    /// One entry per listening process, ordered by lowest port then PID.
    pub services: Vec<Service>,
    /// Every project a service runs in, ordered by root, each once.
    pub projects: Vec<Project>,
    /// What this observation could not see. Empty only when nothing was missed.
    pub warnings: Vec<ScanWarning>,
}

/// How the scanner behind a snapshot is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScannerStatus {
    /// Target time between scans; `null` for a one-shot scan (`kosse list`).
    #[ts(type = "number | null")]
    pub interval_ms: Option<u64>,
    /// Start of the most recent attempt, successful or not, in Unix milliseconds.
    #[ts(type = "number")]
    pub last_attempt_at_ms: u64,
    pub last_attempt_duration_ms: f64,
    /// Why the most recent attempt failed. When set, the observation (if any) is older than
    /// `lastAttemptAtMs`: the last good one, kept rather than replaced by an empty result.
    pub failure: Option<String>,
    pub consecutive_failures: u32,
    /// Attempts so far that took longer than `intervalMs`. The next scan then waits a full
    /// interval after the overrun instead of running back to back.
    #[ts(type = "number")]
    pub overruns: u64,
}

/// A process and every TCP port it listens on. One process is one action target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub process: ProcessRecord,
    /// Ordered by port; each port appears once.
    pub endpoints: Vec<Endpoint>,
    /// `Project.root` of the project this process runs in; `null` when its working directory is
    /// unknown, unresolvable (see warnings) or not inside a project.
    pub project: Option<String>,
    /// The nearest package manifest directory at or above the working directory, within the
    /// project.
    pub package: Option<PackageLabel>,
}

/// A worktree (or, outside Git, a package directory) that services run in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Canonical absolute path of the Git worktree root, or of the package directory when the
    /// service is not in a Git worktree: the project's identity. Separate worktrees of one
    /// repository are separate projects, even when they share a name.
    pub root: String,
    /// Display name: the root directory's name. Not unique; `root` is.
    pub name: String,
    /// `null` outside a Git worktree.
    pub git: Option<GitWorktree>,
}

/// Git facts about a project's worktree, as reported by `git` itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktree {
    /// Canonical path of the repository's common Git directory. Every worktree of one
    /// repository shares it; it groups projects for display and never merges them.
    pub common_dir: String,
    /// The repository's name, derived from the common directory.
    pub repository_name: String,
    /// Added with `git worktree add` (its `.git` file points into the common directory).
    pub linked: bool,
    pub head: GitHead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GitHead {
    /// On a branch (possibly one without commits yet).
    Branch { name: String },
    /// Detached at this abbreviated commit.
    Detached { commit: String },
    /// `git` could not report it.
    Unknown,
}

/// A package inside a project, named after its directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackageLabel {
    /// `/`-separated path relative to the project root; empty for the root itself.
    pub path: String,
    /// The package directory's name.
    pub name: String,
    /// Manifest files present in that directory, in a fixed order.
    pub manifests: Vec<Manifest>,
}

/// Recognised package manifests. Only their presence is checked; contents are never read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub enum Manifest {
    #[serde(rename = "package.json")]
    PackageJson,
    #[serde(rename = "deno.json")]
    DenoJson,
    #[serde(rename = "Cargo.toml")]
    CargoToml,
    #[serde(rename = "pyproject.toml")]
    PyprojectToml,
    #[serde(rename = "go.mod")]
    GoMod,
    #[serde(rename = "Gemfile")]
    Gemfile,
    #[serde(rename = "composer.json")]
    ComposerJson,
    #[serde(rename = "mix.exs")]
    MixExs,
    #[serde(rename = "pom.xml")]
    PomXml,
    #[serde(rename = "build.gradle")]
    BuildGradle,
    #[serde(rename = "build.gradle.kts")]
    BuildGradleKts,
    #[serde(rename = "Package.swift")]
    PackageSwift,
}

impl Manifest {
    pub const ALL: [Self; 12] = [
        Self::PackageJson,
        Self::DenoJson,
        Self::CargoToml,
        Self::PyprojectToml,
        Self::GoMod,
        Self::Gemfile,
        Self::ComposerJson,
        Self::MixExs,
        Self::PomXml,
        Self::BuildGradle,
        Self::BuildGradleKts,
        Self::PackageSwift,
    ];

    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            Self::PackageJson => "package.json",
            Self::DenoJson => "deno.json",
            Self::CargoToml => "Cargo.toml",
            Self::PyprojectToml => "pyproject.toml",
            Self::GoMod => "go.mod",
            Self::Gemfile => "Gemfile",
            Self::ComposerJson => "composer.json",
            Self::MixExs => "mix.exs",
            Self::PomXml => "pom.xml",
            Self::BuildGradle => "build.gradle",
            Self::BuildGradleKts => "build.gradle.kts",
            Self::PackageSwift => "Package.swift",
        }
    }
}

/// A PID plus the evidence that it still names the same process. A PID alone can be recycled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Process start time in Unix seconds; `null` when the OS would not disclose it. Without it
    /// the identity is not eligible for destructive actions.
    #[ts(type = "number | null")]
    pub started_at_secs: Option<u64>,
}

/// What was observed about a listening process. `null` means unknown, never a guess.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRecord {
    pub identity: ProcessIdentity,
    /// Short process name as reported by the OS: untrusted text, truncated by the kernel to 31
    /// bytes and decoded lossily (U+FFFD for a cut character).
    pub name: Option<String>,
    pub executable: Option<String>,
    /// The argument vector after central redaction (see `redact`); `null` when the OS withheld
    /// it. Untrusted text: display it, never execute it.
    pub command: Option<CommandLine>,
    pub cwd: Option<String>,
    pub parent_pid: Option<u32>,
    /// Real user ID of the process owner.
    pub user_id: Option<u32>,
    /// Seconds between the process start and the scan; `null` when the start time is unknown.
    #[ts(type = "number | null")]
    pub age_secs: Option<u64>,
    pub metrics: ProcessMetrics,
}

/// A redacted command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandLine {
    /// Arguments, `argv[0]` first, with secret values replaced by `[redacted]`.
    pub args: Vec<String>,
    /// At least one value was hidden.
    pub redacted: bool,
    /// Arguments or characters beyond the display limits were dropped (after redaction).
    pub truncated: bool,
}

/// Resource use of a process at scan time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMetrics {
    pub cpu: CpuUsage,
    /// Resident memory in bytes; `null` when the OS withheld it.
    #[ts(type = "number | null")]
    pub memory_bytes: Option<u64>,
}

/// CPU use needs two observations of the same process; it is never reported as a made-up zero.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CpuUsage {
    /// First observation of this process identity: nothing to compare with yet.
    WarmingUp,
    /// Average over the time since an earlier observation, as a percentage of one core (a
    /// process busy on two cores reads 200).
    Sampled { percent: f64 },
    /// The OS withheld the process's CPU time (usually another user's process).
    Unavailable,
}

/// One port of a service, with every address it is bound to on that port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub protocol: TransportProtocol,
    pub port: u16,
    /// Canonical IP text (IPv4-mapped IPv6 shown as IPv4), IPv4 first, each once. Separate
    /// IPv4/IPv6 sockets on one port share an endpoint but keep their addresses here.
    pub bind_addresses: Vec<String>,
    pub exposure: Exposure,
    /// Bounded HTTP enrichment. Listening is a fact; this only says what an HTTP request saw.
    pub http: HttpProbe,
}

/// Whether an endpoint was probed over HTTP, and what was seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HttpProbe {
    /// Not an HTTP candidate, so never probed.
    Skipped { reason: ProbeSkip },
    /// A candidate without a result yet. A one-shot `kosse list` never probes.
    Pending,
    Probed {
        /// When the request started, in Unix milliseconds. Results are reused for a while
        /// (probing is slower than scanning), so this can be older than the scan.
        #[ts(type = "number")]
        probed_at_ms: u64,
        /// The user asked for this probe (`POST /api/probe`, "try HTTP") on an endpoint that is
        /// not probed automatically; `false` for automatic probes.
        requested: bool,
        result: ProbeResult,
    },
}

/// Why an endpoint is not probed automatically. Only the current user's processes that run in
/// a project are probed on their own; `otherUser` and `notOnLoopback` are never probed, the rest
/// can be probed on request ("try HTTP").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProbeSkip {
    /// Bound only to non-loopback addresses; Kossé probes loopback only.
    NotOnLoopback,
    /// Owned by another user or the system (or the owner is unknown).
    OtherUser,
    /// The port of a protocol that is not HTTP (databases, mail, SSH, …).
    KnownNonHttpPort,
    /// The process is a known non-HTTP server (databases, caches, SSH, …).
    KnownNonHttpProcess,
    /// The process does not run in a project, so it is not presumed to be a dev server.
    NotAttributed,
}

impl ProbeSkip {
    /// Whether the user may still ask for a probe of an endpoint skipped for this reason.
    #[must_use]
    pub fn allows_request(self) -> bool {
        !matches!(self, Self::NotOnLoopback | Self::OtherUser)
    }
}

/// The outcome of one bounded `GET /`. Redirects are recorded, never followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProbeResult {
    /// An HTTP response arrived: the service is reachable HTTP, whatever the status (404 and
    /// 500 included).
    Response {
        /// The `http://` or `https://` loopback URL that answered, for opening the service.
        url: String,
        status: u16,
        /// The HTML `<title>`, whitespace-collapsed; untrusted text.
        title: Option<String>,
        /// A redirect's `Location` header, redacted and as sent: untrusted text, not a link.
        location: Option<String>,
        /// The response arrived over TLS whose certificate verified against the keychains' trusted
        /// roots (checked offline, in process).
        tls: bool,
        /// Only the first 64 KiB of the body were read.
        body_truncated: bool,
    },
    /// No response within the time limit (the connection may have been accepted).
    TimedOut,
    /// The connection was refused: nothing listens there any more.
    Refused,
    /// The connection was accepted, then closed or reset without any answer (over plain HTTP
    /// and over TLS).
    Closed,
    /// The endpoint speaks TLS, but the handshake or certificate check failed (for example a
    /// self-signed certificate). Verification is never disabled.
    TlsFailed {
        failure: TlsFailure,
        /// The TLS library's description: untrusted text (it can quote the peer's certificate).
        reason: String,
    },
    /// The endpoint answered with something other than HTTP, or spoke before being asked (as
    /// SSH, SMTP or MySQL do); no request was sent then.
    NotHttp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransportProtocol {
    Tcp,
}

/// Who can reach an endpoint, judged from its bind addresses alone (no firewall knowledge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Exposure {
    /// Every bind address is a loopback address.
    Loopback,
    /// Bound to at least one specific non-loopback address.
    Interface,
    /// Bound to a wildcard address (`0.0.0.0` or `::`): every interface.
    AllInterfaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TlsFailure {
    /// The certificate was not accepted: untrusted issuer, wrong name, expired, ….
    Certificate,
    /// The TLS handshake itself failed (protocol version, cipher suites, alerts, malformed
    /// records).
    Handshake,
}

/// Partial data in a snapshot: something that exists but could not be observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScanWarning {
    /// Without administrator privileges macOS hides other users' sockets (system services
    /// included), so their listeners are absent rather than reported.
    OtherUsersNotVisible,
    /// A listening socket was seen, but its process ended before it could be inspected.
    ProcessExited { pid: u32, ports: Vec<u16> },
    /// The process was inspected but the OS withheld some fields (usually permissions).
    MetadataUnavailable { pid: u32, fields: Vec<ProcessField> },
    /// The process reported a working directory, but its project could not be determined.
    AttributionUnavailable {
        pid: u32,
        reason: AttributionFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AttributionFailure {
    /// The working directory could not be resolved (for example, it was deleted).
    CwdUnavailable,
    /// The directory is inside a Git worktree, but no usable `git` is installed (Xcode Command
    /// Line Tools).
    GitUnavailable,
    /// `git` did not answer in time.
    GitTimedOut,
    /// `git` failed or gave an answer that does not match the directory layout.
    GitFailed,
    /// This scan's time budget for `git` was spent; a later scan resolves it.
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProcessField {
    Name,
    StartTime,
    User,
    Command,
    Cwd,
    Cpu,
    Memory,
}

impl fmt::Display for ScanWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherUsersNotVisible => f.write_str(
                "listeners owned by other users (including system services) are not visible without administrator privileges",
            ),
            Self::ProcessExited { pid, ports } => {
                write!(f, "process {pid} exited during the scan (ports ")?;
                write_list(f, ports)?;
                f.write_str(")")
            }
            Self::MetadataUnavailable { pid, fields } => {
                write!(f, "process {pid}: ")?;
                write_list(f, fields)?;
                f.write_str(" unavailable")
            }
            Self::AttributionUnavailable { pid, reason } => {
                write!(f, "process {pid}: project unknown ({reason})")
            }
        }
    }
}

impl fmt::Display for ProcessField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Name => "name",
            Self::StartTime => "start time",
            Self::User => "user",
            Self::Command => "command line",
            Self::Cwd => "working directory",
            Self::Cpu => "CPU",
            Self::Memory => "memory",
        })
    }
}

impl fmt::Display for AttributionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CwdUnavailable => "working directory unavailable",
            Self::GitUnavailable => "git is not installed",
            Self::GitTimedOut => "git timed out",
            Self::GitFailed => "git failed",
            Self::Deferred => "deferred to a later scan",
        })
    }
}

fn write_list<T: fmt::Display>(f: &mut fmt::Formatter<'_>, items: &[T]) -> fmt::Result {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}
