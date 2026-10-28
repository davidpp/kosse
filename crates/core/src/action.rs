//! The stop policy shared by every view: which process may be signalled, what a confirmation
//! must name, and how to read what happened afterwards. Decisions only; the signal itself is in
//! [`crate::signal`] and the timing of the observation in the app.
//!
//! A request names the process identity (PID + start time) and every port the user confirmed,
//! as seen in a snapshot of this instance. Before any signal:
//!
//! 1. [`revalidate`] judges a fresh look at that one PID ([`crate::inspect::vitals`]): not PID 0
//!    or 1, not Kossé itself, owned (real and effective user) by the user Kossé runs as, the
//!    same start time. Only this produces an [`Eligible`] target, the one input
//!    [`crate::signal::send`] takes.
//! 2. [`confirm`] checks that the identity is in the current observation and that the confirmed
//!    ports are exactly the ports it listens on now (from a socket enumeration made just before
//!    the fresh look).
//!
//! After the signal, [`after_exit`] classifies who holds the confirmed ports once the process
//! is gone, telling processes that already shared those port numbers before the signal apart
//! from new ones; an observation failure is never read as success.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    inspect::Presence,
    model::{Observation, ProcessIdentity},
};

/// `POST /api/stop` (graceful, `SIGTERM`) and `POST /api/force` (`SIGKILL`, only after a
/// `stillRunning` result for the same identity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionRequest {
    /// `Snapshot.instance` of the snapshot the user confirmed from.
    pub instance: String,
    pub identity: ProcessIdentity,
    /// Every port the confirmation showed. It must be exactly the ports this identity listens
    /// on in the current snapshot, so nobody stops a process without seeing all it serves.
    pub ports: Vec<u16>,
}

/// The outcome of a stop or force request. Only `exited` means the process is gone and its
/// ports are free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ActionResult {
    /// The process exited and no new process listened on its ports afterwards. `shared` lists
    /// processes that already listened on the same port numbers before the signal (another
    /// address, or a socket shared across `fork`) and still do; empty means the ports are free.
    Exited {
        target: ProcessIdentity,
        ports: Vec<u16>,
        shared: Vec<PortOwner>,
    },
    /// Still running when the observation window ended (it may ignore `SIGTERM`). Force is
    /// offered for this identity until `forceUntilMs` (Unix milliseconds), and never sent
    /// automatically; `null` when the process could not be confirmed as the same one (another
    /// parent), so no force is offered.
    StillRunning {
        target: ProcessIdentity,
        ports: Vec<u16>,
        #[ts(type = "number | null")]
        force_until_ms: Option<u64>,
    },
    /// The process exited, and a new process with the same parent (`parentPid`, likely a
    /// supervisor) listens on its ports again. Its parent was not signalled.
    Respawned {
        target: ProcessIdentity,
        parent_pid: u32,
        owners: Vec<PortOwner>,
        shared: Vec<PortOwner>,
    },
    /// The process exited, and a new process (not a sibling) now listens on its ports.
    EndpointTaken {
        target: ProcessIdentity,
        owners: Vec<PortOwner>,
        shared: Vec<PortOwner>,
    },
    /// The signal was sent, but what followed could not be observed; never a success.
    ObservationFailed {
        target: ProcessIdentity,
        /// The process was seen gone; only its ports could not be checked.
        process_exited: bool,
        reason: String,
    },
    /// The confirmation no longer matches what runs; nothing was signalled. Refresh and confirm
    /// again.
    Stale { reason: StaleReason },
    /// Force needs a `stillRunning` result for this identity first (offers expire); nothing was
    /// signalled.
    ForceNotOffered,
    /// Kossé never signals this process; nothing was signalled.
    Refused { reason: Ineligible },
    /// The OS refused the signal.
    PermissionDenied { target: ProcessIdentity },
    /// The checks before the signal or the `kill(2)` call itself failed unexpectedly; nothing
    /// was signalled.
    SignalFailed {
        target: ProcessIdentity,
        reason: String,
    },
    /// Kossé failed while handling the request; whether a signal was sent is unknown. Refresh.
    Internal { reason: String },
}

/// A process listening on one of the target's port numbers. `identity.startedAtSecs` is
/// `null` when the OS disclosed nothing about it (it is still listed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PortOwner {
    pub port: u16,
    pub identity: ProcessIdentity,
    /// Untrusted text, as reported by the OS.
    pub name: Option<String>,
    pub parent_pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StaleReason {
    /// The confirmation came from another (earlier) Kossé instance.
    InstanceChanged,
    /// The process has already exited.
    Exited,
    /// The PID now names a different process (another start time).
    IdentityChanged,
    /// The identity is not in the current snapshot.
    NotInSnapshot,
    /// The process listens on other ports than the ones confirmed.
    PortsChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Ineligible {
    /// PID 0 or 1 (the kernel, launchd).
    SystemProcess,
    /// Kossé itself; quit it instead.
    KosseItself,
    /// Without a start time a reused PID cannot be told apart.
    NoStartTime,
    /// Another user's (or the system's) process, or one running with another effective user.
    OtherUser,
    /// The OS did not disclose the owner. Without administrator rights macOS withholds it (and
    /// the start time) for other users' and system processes.
    OwnerUnknown,
    /// The process exists, but the OS returned nothing about it.
    Unreadable,
    /// Debug builds started with `--act-only` (tests) signal only those processes and their
    /// children.
    OutsideTestScope,
}

/// Why a request does not reach the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Stale(StaleReason),
    Ineligible(Ineligible),
}

impl From<Rejection> for ActionResult {
    fn from(rejection: Rejection) -> Self {
        match rejection {
            Rejection::Stale(reason) => Self::Stale { reason },
            Rejection::Ineligible(reason) => Self::Refused { reason },
        }
    }
}

/// Who acts: this Kossé process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    pub pid: u32,
    pub effective_user: u32,
}

/// A process that passed [`revalidate`]: PID above 1, not the actor, start time known and
/// matching, owned by the actor's user. Only [`revalidate`] creates one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eligible {
    identity: ProcessIdentity,
    started_at_secs: u64,
    parent_pid: Option<u32>,
}

impl Eligible {
    #[must_use]
    pub fn identity(&self) -> ProcessIdentity {
        self.identity
    }

    #[must_use]
    pub fn pid(&self) -> u32 {
        self.identity.pid
    }

    #[must_use]
    pub fn started_at_secs(&self) -> u64 {
        self.started_at_secs
    }

    /// The parent at revalidation: it may explain a respawn, never authorizes signalling it.
    #[must_use]
    pub fn parent_pid(&self) -> Option<u32> {
        self.parent_pid
    }
}

/// Judges a fresh look (`presence`) at the requested PID.
pub fn revalidate(
    requested: ProcessIdentity,
    presence: &Presence,
    actor: &Actor,
) -> Result<Eligible, Rejection> {
    let ineligible = |reason| Err(Rejection::Ineligible(reason));
    if requested.pid <= 1 {
        return ineligible(Ineligible::SystemProcess);
    }
    if requested.pid == actor.pid {
        return ineligible(Ineligible::KosseItself);
    }
    let Some(requested_start) = requested.started_at_secs else {
        return ineligible(Ineligible::NoStartTime);
    };
    let vitals = match presence {
        Presence::Exited => return Err(Rejection::Stale(StaleReason::Exited)),
        Presence::Unknown => return ineligible(Ineligible::Unreadable),
        Presence::Running(vitals) => vitals,
    };
    // Owner first: macOS discloses neither owner nor start time of other users' processes.
    match (vitals.user_id, vitals.effective_user_id) {
        (Some(real), Some(effective))
            if real == actor.effective_user && effective == actor.effective_user => {}
        (Some(_), Some(_)) => return ineligible(Ineligible::OtherUser),
        _ => return ineligible(Ineligible::OwnerUnknown),
    }
    let Some(started_at_secs) = vitals.started_at_secs else {
        return ineligible(Ineligible::NoStartTime);
    };
    if started_at_secs != requested_start {
        return Err(Rejection::Stale(StaleReason::IdentityChanged));
    }
    Ok(Eligible {
        identity: requested,
        started_at_secs,
        parent_pid: vitals.parent_pid,
    })
}

/// The confirmation matches: the identity is in the current observation, and `live_ports` (the
/// ports it listens on now, from a fresh enumeration) are exactly the confirmed ones. Returns
/// those ports, ordered.
pub fn confirm(
    observation: &Observation,
    identity: ProcessIdentity,
    confirmed: &[u16],
    live_ports: &[u16],
) -> Result<Vec<u16>, StaleReason> {
    if !observation
        .services
        .iter()
        .any(|service| service.process.identity == identity)
    {
        return Err(StaleReason::NotInSnapshot);
    }
    let ordered = |ports: &[u16]| {
        let mut ports = ports.to_vec();
        ports.sort_unstable();
        ports.dedup();
        ports
    };
    let current = ordered(live_ports);
    if ordered(confirmed) == current {
        Ok(current)
    } else {
        Err(StaleReason::PortsChanged)
    }
}

/// The target's ports once it has exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortsAfterExit {
    /// No new owner (so far); `shared` are the pre-existing owners still listening.
    Free {
        shared: Vec<PortOwner>,
    },
    Decided(ActionResult),
}

/// Classifies the target's ports once it has exited. `owners` are the processes listening on
/// them now (or why that could not be read); `before` are the other processes that already
/// listened on those port numbers just before the signal. Only owners not in `before` count as
/// a respawn or a takeover.
#[must_use]
pub fn after_exit(
    target: &Eligible,
    owners: Result<Vec<PortOwner>, String>,
    before: &[PortOwner],
) -> PortsAfterExit {
    let owners = match owners {
        Ok(owners) => owners,
        Err(reason) => {
            return PortsAfterExit::Decided(ActionResult::ObservationFailed {
                target: target.identity,
                process_exited: true,
                reason,
            });
        }
    };
    let (shared, owners): (Vec<PortOwner>, Vec<PortOwner>) = owners
        .into_iter()
        .partition(|owner| before.iter().any(|old| old.identity == owner.identity));
    if owners.is_empty() {
        return PortsAfterExit::Free { shared };
    }
    // A shared parent other than launchd suggests a supervisor restarted it.
    let sibling_parent = target.parent_pid.filter(|&parent| parent > 1);
    PortsAfterExit::Decided(match sibling_parent {
        Some(parent) if owners.iter().all(|owner| owner.parent_pid == Some(parent)) => {
            ActionResult::Respawned {
                target: target.identity,
                parent_pid: parent,
                owners,
                shared,
            }
        }
        _ => ActionResult::EndpointTaken {
            target: target.identity,
            owners,
            shared,
        },
    })
}

/// Whether a process still running after the window may be offered force: the same start time
/// and the same parent as at revalidation, so a PID reused within the start time's one-second
/// resolution does not inherit the offer.
#[must_use]
pub fn may_offer_force(target: &Eligible, now: &Presence) -> bool {
    matches!(now, Presence::Running(vitals)
        if vitals.started_at_secs == Some(target.started_at_secs)
            && vitals.parent_pid == target.parent_pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspect::ProcessVitals,
        model::{
            CpuUsage, Endpoint, Exposure, HttpProbe, ProcessMetrics, ProcessRecord, Service,
            TransportProtocol,
        },
    };

    const ME: Actor = Actor {
        pid: 500,
        effective_user: 501,
    };

    fn id(pid: u32, started: Option<u64>) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started_at_secs: started,
        }
    }

    fn running(started: Option<u64>, user: Option<u32>, effective: Option<u32>) -> Presence {
        Presence::Running(ProcessVitals {
            started_at_secs: started,
            parent_pid: Some(42),
            user_id: user,
            effective_user_id: effective,
        })
    }

    fn mine(started: u64) -> Presence {
        running(Some(started), Some(501), Some(501))
    }

    fn refused(reason: Ineligible) -> Result<Eligible, Rejection> {
        Err(Rejection::Ineligible(reason))
    }

    #[test]
    fn only_own_user_processes_with_a_matching_start_are_eligible() {
        let eligible = revalidate(id(900, Some(10)), &mine(10), &ME).expect("eligible");
        assert_eq!(eligible.identity(), id(900, Some(10)));
        assert_eq!(eligible.parent_pid(), Some(42));

        assert_eq!(
            revalidate(id(900, Some(10)), &mine(11), &ME),
            Err(Rejection::Stale(StaleReason::IdentityChanged))
        );
        assert_eq!(
            revalidate(id(900, Some(10)), &Presence::Exited, &ME),
            Err(Rejection::Stale(StaleReason::Exited))
        );
        assert_eq!(
            revalidate(id(900, Some(10)), &running(Some(10), Some(0), Some(0)), &ME),
            refused(Ineligible::OtherUser)
        );
        // Started by this user but running setuid as another.
        assert_eq!(
            revalidate(
                id(900, Some(10)),
                &running(Some(10), Some(501), Some(0)),
                &ME
            ),
            refused(Ineligible::OtherUser)
        );
        assert_eq!(
            revalidate(id(900, Some(10)), &running(Some(10), None, Some(501)), &ME),
            refused(Ineligible::OwnerUnknown)
        );
        // What macOS discloses about another user's process to a non-administrator.
        assert_eq!(
            revalidate(id(900, Some(10)), &running(None, None, None), &ME),
            refused(Ineligible::OwnerUnknown)
        );
        assert_eq!(
            revalidate(id(900, Some(10)), &running(None, Some(501), Some(501)), &ME),
            refused(Ineligible::NoStartTime)
        );
    }

    #[test]
    fn system_self_and_identities_without_a_start_are_refused_before_looking() {
        for pid in [0, 1] {
            assert_eq!(
                revalidate(id(pid, Some(10)), &mine(10), &ME),
                refused(Ineligible::SystemProcess)
            );
        }
        assert_eq!(
            revalidate(id(ME.pid, Some(10)), &mine(10), &ME),
            refused(Ineligible::KosseItself)
        );
        assert_eq!(
            revalidate(id(900, None), &mine(10), &ME),
            refused(Ineligible::NoStartTime)
        );
    }

    fn service(identity: ProcessIdentity, ports: &[u16]) -> Service {
        Service {
            process: ProcessRecord {
                identity,
                name: None,
                executable: None,
                command: None,
                cwd: None,
                parent_pid: None,
                user_id: Some(501),
                age_secs: None,
                metrics: ProcessMetrics {
                    cpu: CpuUsage::WarmingUp,
                    memory_bytes: None,
                },
            },
            endpoints: ports
                .iter()
                .map(|&port| Endpoint {
                    protocol: TransportProtocol::Tcp,
                    port,
                    bind_addresses: vec!["127.0.0.1".to_owned()],
                    exposure: Exposure::Loopback,
                    http: HttpProbe::Pending,
                })
                .collect(),
            project: None,
            package: None,
        }
    }

    #[test]
    fn a_confirmation_must_name_every_live_port() {
        let target = id(900, Some(10));
        let observation = Observation {
            scanned_at_ms: 0,
            scan_duration_ms: 0.0,
            services: vec![service(target, &[3000, 3001])],
            projects: Vec::new(),
            warnings: Vec::new(),
        };
        let live = [3001, 3000];
        assert_eq!(
            confirm(&observation, target, &[3001, 3000], &live),
            Ok(vec![3000, 3001])
        );
        assert_eq!(
            confirm(&observation, target, &[3000], &live),
            Err(StaleReason::PortsChanged)
        );
        assert_eq!(
            confirm(&observation, target, &[3000, 3001, 3002], &live),
            Err(StaleReason::PortsChanged)
        );
        // A port opened after the last scan is not in the confirmation.
        assert_eq!(
            confirm(&observation, target, &[3000, 3001], &[3000, 3001, 3002]),
            Err(StaleReason::PortsChanged)
        );
        assert_eq!(
            confirm(&observation, id(900, Some(11)), &[3000, 3001], &live),
            Err(StaleReason::NotInSnapshot)
        );
    }

    fn owner(port: u16, pid: u32, parent: Option<u32>) -> PortOwner {
        PortOwner {
            port,
            identity: id(pid, Some(20)),
            name: None,
            parent_pid: parent,
        }
    }

    #[test]
    fn ports_after_exit_distinguish_free_shared_respawned_taken_and_unknown() {
        let target = revalidate(id(900, Some(10)), &mine(10), &ME).expect("eligible");
        let free = |shared| PortsAfterExit::Free { shared };
        assert_eq!(after_exit(&target, Ok(Vec::new()), &[]), free(Vec::new()));
        // Already on that port number before the signal: shared, not a successor.
        let neighbour = owner(3000, 800, Some(7));
        assert_eq!(
            after_exit(
                &target,
                Ok(vec![neighbour.clone()]),
                std::slice::from_ref(&neighbour)
            ),
            free(vec![neighbour.clone()])
        );
        assert_eq!(
            after_exit(
                &target,
                Ok(vec![neighbour.clone(), owner(3000, 901, Some(42))]),
                std::slice::from_ref(&neighbour)
            ),
            PortsAfterExit::Decided(ActionResult::Respawned {
                target: target.identity(),
                parent_pid: 42,
                owners: vec![owner(3000, 901, Some(42))],
                shared: vec![neighbour],
            })
        );
        let mixed = vec![owner(3000, 901, Some(42)), owner(3001, 777, Some(1))];
        assert_eq!(
            after_exit(&target, Ok(mixed.clone()), &[]),
            PortsAfterExit::Decided(ActionResult::EndpointTaken {
                target: target.identity(),
                owners: mixed,
                shared: Vec::new(),
            })
        );
        // An owner the OS disclosed nothing about is still an owner.
        let unreadable = PortOwner {
            identity: id(950, None),
            ..owner(3000, 950, None)
        };
        assert!(matches!(
            after_exit(&target, Ok(vec![unreadable]), &[]),
            PortsAfterExit::Decided(ActionResult::EndpointTaken { .. })
        ));
        assert_eq!(
            after_exit(&target, Err("enumeration failed".to_owned()), &[]),
            PortsAfterExit::Decided(ActionResult::ObservationFailed {
                target: target.identity(),
                process_exited: true,
                reason: "enumeration failed".to_owned(),
            })
        );
    }

    #[test]
    fn force_is_offered_only_to_the_same_start_and_parent() {
        let target = revalidate(id(900, Some(10)), &mine(10), &ME).expect("eligible");
        assert!(may_offer_force(&target, &mine(10)));
        assert!(!may_offer_force(&target, &mine(11)));
        let reparented = Presence::Running(ProcessVitals {
            started_at_secs: Some(10),
            parent_pid: Some(43),
            user_id: Some(501),
            effective_user_id: Some(501),
        });
        assert!(!may_offer_force(&target, &reparented));
        assert!(!may_offer_force(&target, &Presence::Unknown));
        assert!(!may_offer_force(&target, &Presence::Exited));
    }

    #[test]
    fn an_unreadable_process_is_refused_not_stale() {
        assert_eq!(
            revalidate(id(900, Some(10)), &Presence::Unknown, &ME),
            refused(Ineligible::Unreadable)
        );
    }

    #[test]
    fn a_launchd_parent_never_explains_a_respawn() {
        let orphan = revalidate(
            id(900, Some(10)),
            &Presence::Running(ProcessVitals {
                started_at_secs: Some(10),
                parent_pid: Some(1),
                user_id: Some(501),
                effective_user_id: Some(501),
            }),
            &ME,
        )
        .expect("eligible");
        assert!(matches!(
            after_exit(&orphan, Ok(vec![owner(3000, 901, Some(1))]), &[]),
            PortsAfterExit::Decided(ActionResult::EndpointTaken { .. })
        ));
    }
}
