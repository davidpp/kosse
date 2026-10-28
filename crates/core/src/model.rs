//! The observed service model shared by every view. These types are wire contracts: TypeScript
//! declarations are generated from them (see [`crate::contract`]).

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped when a change to [`Snapshot`] would break an existing reader.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// One observation of the machine's TCP listeners and the processes that own them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    /// Wall-clock time the scan started, in Unix milliseconds.
    #[ts(type = "number")]
    pub scanned_at_ms: u64,
    pub scan_duration_ms: f64,
    /// One entry per listening process, ordered by lowest port then PID.
    pub services: Vec<Service>,
    /// What this snapshot could not observe. Empty only when nothing was missed.
    pub warnings: Vec<ScanWarning>,
}

/// A process and every TCP port it listens on. One process is one action target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub process: ProcessRecord,
    /// Ordered by port; each port appears once.
    pub endpoints: Vec<Endpoint>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRecord {
    pub identity: ProcessIdentity,
    /// Short process name as reported by the OS: untrusted text, truncated by the kernel to 31
    /// bytes and decoded lossily (U+FFFD for a cut character).
    pub name: Option<String>,
    pub executable: Option<String>,
    pub cwd: Option<String>,
    pub parent_pid: Option<u32>,
    /// Real user ID of the process owner.
    pub user_id: Option<u32>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProcessField {
    Name,
    StartTime,
    User,
    Cwd,
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
        }
    }
}

impl fmt::Display for ProcessField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Name => "name",
            Self::StartTime => "start time",
            Self::User => "user",
            Self::Cwd => "working directory",
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
