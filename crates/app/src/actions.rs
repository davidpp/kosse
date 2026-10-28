//! Executes stop and force requests with the core policy ([`kosse_core::action`]): enumerate the
//! listening sockets (the target's live ports and who else is on those port numbers), read the
//! one PID fresh and revalidate it, check the confirmation against the current snapshot and the
//! live ports, send one signal, then observe the process and its ports for a bounded time. Runs
//! on a blocking thread; one action at a time.
//!
//! Force (`SIGKILL`) is accepted only for an identity whose stop ended `stillRunning` within
//! [`FORCE_OFFER_TTL`]; each offer is used once. Nothing escalates on its own.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use kosse_core::{
    action::{
        self, ActionRequest, ActionResult, Actor, Eligible, Ineligible, PortOwner, PortsAfterExit,
        StaleReason,
    },
    inspect::{self, Presence},
    model::{ProcessIdentity, Snapshot},
    scan::unix_millis,
    signal::{self, Signal, SignalError},
    sockets::{self, ListenSocket},
};

/// How long a `SIGTERM`ed process gets to exit before the result is `stillRunning`.
const TERM_WINDOW: Duration = Duration::from_secs(5);
/// `SIGKILL` cannot be ignored; this only covers a process stuck in the kernel.
const KILL_WINDOW: Duration = Duration::from_secs(2);
/// After the process exits, how long its ports are watched for a respawn before they are
/// reported free. A supervisor usually restarts its child well within this.
const RESPAWN_GRACE: Duration = Duration::from_secs(1);
/// How long a `stillRunning` result keeps force available for that identity.
pub const FORCE_OFFER_TTL: Duration = Duration::from_secs(60);
const FIRST_POLL: Duration = Duration::from_millis(20);
const MAX_POLL: Duration = Duration::from_millis(200);
const PORT_POLL: Duration = Duration::from_millis(100);

/// Which processes may be signalled at all.
pub enum Scope {
    /// Every eligible process (production).
    All,
    /// Only these processes (PID → start time, read at startup) and their direct children
    /// (supervised fixtures). Debug builds' `--act-only`, so a test can never signal one of the
    /// machine's real services.
    #[cfg_attr(
        not(debug_assertions),
        expect(
            dead_code,
            reason = "only the debug-only `--act-only` flag constructs it"
        )
    )]
    Only(HashMap<u32, u64>),
}

impl Scope {
    /// Records each PID's current start time; a PID without one is left out (never signalled).
    #[cfg_attr(
        not(debug_assertions),
        expect(dead_code, reason = "only the debug-only `--act-only` flag uses it")
    )]
    pub fn only(pids: &[u32]) -> Self {
        Self::Only(
            pids.iter()
                .filter_map(|&pid| match inspect::vitals(pid) {
                    Presence::Running(vitals) => vitals.started_at_secs.map(|start| (pid, start)),
                    Presence::Exited | Presence::Unknown => None,
                })
                .collect(),
        )
    }

    /// `parent` is the target's parent PID and start time as read before the target itself.
    fn allows(&self, target: &Eligible, parent: Option<(u32, Option<u64>)>) -> bool {
        match self {
            Self::All => true,
            Self::Only(processes) => {
                let listed = |pid, start| processes.get(&pid) == Some(&start);
                listed(target.pid(), target.started_at_secs())
                    || matches!(parent, Some((pid, Some(start)))
                        if target.parent_pid() == Some(pid) && listed(pid, start))
            }
        }
    }
}

struct ForceOffer {
    ports: Vec<u16>,
    until: Instant,
}

pub struct Actions {
    /// `None` when Kossé could not read its own effective user: then nothing is eligible.
    actor: Option<Actor>,
    scope: Scope,
    offers: Mutex<HashMap<ProcessIdentity, ForceOffer>>,
    busy: AtomicBool,
}

/// Held while an action runs; dropping it lets the next one start.
pub struct Busy(Arc<Actions>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

impl Actions {
    pub fn new(scope: Scope) -> Self {
        let pid = std::process::id();
        let actor = match inspect::vitals(pid) {
            Presence::Running(vitals) => vitals.effective_user_id.map(|effective_user| Actor {
                pid,
                effective_user,
            }),
            Presence::Exited | Presence::Unknown => None,
        };
        Self {
            actor,
            scope,
            offers: Mutex::default(),
            busy: AtomicBool::new(false),
        }
    }

    /// Claims the single action slot; `None` while another action runs.
    pub fn begin(self: &Arc<Self>) -> Option<Busy> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Busy(Arc::clone(self)))
    }

    /// Blocking: revalidates, signals and observes. `snapshot` is the latest publication.
    pub fn execute(
        &self,
        request: &ActionRequest,
        snapshot: &Snapshot,
        signal: Signal,
    ) -> ActionResult {
        if request.instance != snapshot.instance {
            return ActionResult::Stale {
                reason: StaleReason::InstanceChanged,
            };
        }
        if signal == Signal::Kill && !self.has_offer(request) {
            return ActionResult::ForceNotOffered;
        }
        let Some(actor) = self.actor else {
            return ActionResult::Refused {
                reason: Ineligible::OwnerUnknown,
            };
        };
        let Some(observation) = snapshot.observation.as_ref() else {
            return ActionResult::Stale {
                reason: StaleReason::NotInSnapshot,
            };
        };

        // Everything that needs I/O besides the target's own fresh read happens first.
        let parent = self.scope_parent(observation, request.identity);
        let sockets = match sockets::tcp_listen_sockets() {
            Ok(sockets) => sockets,
            Err(error) => {
                return ActionResult::SignalFailed {
                    target: request.identity,
                    reason: error.to_string(),
                };
            }
        };
        let live_ports: Vec<u16> = sockets
            .iter()
            .filter(|socket| socket.pid == request.identity.pid)
            .map(|socket| socket.port)
            .collect();
        let shared_before = owners_on(&sockets, &request.ports, |identity| {
            identity.pid == request.identity.pid
        });

        // From the fresh read to the signal only in-memory checks run, keeping the window in
        // which the PID could be reused as short as possible.
        let presence = inspect::vitals(request.identity.pid);
        let target = match action::revalidate(request.identity, &presence, &actor) {
            Ok(target) => target,
            Err(rejection) => return rejection.into(),
        };
        if !self.scope.allows(&target, parent) {
            return ActionResult::Refused {
                reason: Ineligible::OutsideTestScope,
            };
        }
        let ports =
            match action::confirm(observation, request.identity, &request.ports, &live_ports) {
                Ok(ports) => ports,
                Err(reason) => return ActionResult::Stale { reason },
            };
        if signal == Signal::Kill {
            self.lock_offers().remove(&request.identity);
        }
        match signal::send(&target, signal) {
            Ok(()) => {}
            Err(SignalError::NoSuchProcess) => {
                return ActionResult::Stale {
                    reason: StaleReason::Exited,
                };
            }
            Err(SignalError::PermissionDenied) => {
                return ActionResult::PermissionDenied {
                    target: target.identity(),
                };
            }
            Err(error @ SignalError::Os(_)) => {
                return ActionResult::SignalFailed {
                    target: target.identity(),
                    reason: error.to_string(),
                };
            }
        }

        let window = match signal {
            Signal::Terminate => TERM_WINDOW,
            Signal::Kill => KILL_WINDOW,
        };
        let result = observe(&target, &ports, &shared_before, window);
        if let ActionResult::StillRunning {
            force_until_ms: Some(_),
            ..
        } = result
        {
            self.lock_offers().insert(
                target.identity(),
                ForceOffer {
                    ports,
                    until: Instant::now() + FORCE_OFFER_TTL,
                },
            );
        }
        result
    }

    /// With a test scope: the snapshot's parent of the identity, and that parent's start time.
    fn scope_parent(
        &self,
        observation: &kosse_core::model::Observation,
        identity: ProcessIdentity,
    ) -> Option<(u32, Option<u64>)> {
        if matches!(self.scope, Scope::All) {
            return None;
        }
        let parent = observation
            .services
            .iter()
            .find(|service| service.process.identity == identity)?
            .process
            .parent_pid?;
        let start = match inspect::vitals(parent) {
            Presence::Running(vitals) => vitals.started_at_secs,
            Presence::Exited | Presence::Unknown => None,
        };
        Some((parent, start))
    }

    fn has_offer(&self, request: &ActionRequest) -> bool {
        let mut offers = self.lock_offers();
        let now = Instant::now();
        offers.retain(|_, offer| offer.until > now);
        let confirmed: BTreeSet<u16> = request.ports.iter().copied().collect();
        offers.get(&request.identity).is_some_and(|offer| {
            offer.ports.iter().copied().collect::<BTreeSet<u16>>() == confirmed
        })
    }

    fn lock_offers(&self) -> std::sync::MutexGuard<'_, HashMap<ProcessIdentity, ForceOffer>> {
        // Inserts and removes leave the map consistent, so a poisoned lock is still usable.
        self.offers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Watches the signalled process until it is gone or `window` ends, then its ports for
/// [`RESPAWN_GRACE`]. Unknown is never reported as gone.
fn observe(
    target: &Eligible,
    ports: &[u16],
    shared_before: &[PortOwner],
    window: Duration,
) -> ActionResult {
    let deadline = Instant::now() + window;
    let mut pause = FIRST_POLL;
    loop {
        let presence = inspect::vitals(target.pid());
        match presence {
            Presence::Exited => break,
            // Another start time: the PID was already reused, so the target is gone.
            Presence::Running(vitals)
                if vitals
                    .started_at_secs
                    .is_some_and(|started| started != target.started_at_secs()) =>
            {
                break;
            }
            Presence::Running(_) | Presence::Unknown => {}
        }
        if Instant::now() >= deadline {
            return match presence {
                Presence::Running(vitals)
                    if vitals.started_at_secs == Some(target.started_at_secs()) =>
                {
                    ActionResult::StillRunning {
                        target: target.identity(),
                        ports: ports.to_vec(),
                        force_until_ms: action::may_offer_force(target, &presence)
                            .then(|| unix_millis(SystemTime::now() + FORCE_OFFER_TTL)),
                    }
                }
                _ => ActionResult::ObservationFailed {
                    target: target.identity(),
                    process_exited: false,
                    reason: "the process could no longer be read".to_owned(),
                },
            };
        }
        thread::sleep(pause);
        pause = (pause * 2).min(MAX_POLL);
    }

    let grace_end = Instant::now() + RESPAWN_GRACE;
    loop {
        let owners = sockets::tcp_listen_sockets()
            .map(|sockets| owners_on(&sockets, ports, |identity| *identity == target.identity()))
            .map_err(|error| error.to_string());
        match action::after_exit(target, owners, shared_before) {
            PortsAfterExit::Decided(result) => return result,
            PortsAfterExit::Free { shared } if Instant::now() >= grace_end => {
                return ActionResult::Exited {
                    target: target.identity(),
                    ports: ports.to_vec(),
                    shared,
                };
            }
            PortsAfterExit::Free { .. } => thread::sleep(PORT_POLL),
        }
    }
}

/// Who listens on `ports` (one entry per port and process), except identities `skip` matches.
/// A process the OS discloses nothing about stays in, with an unknown start time; only one
/// confirmed gone is left out.
fn owners_on(
    sockets: &[ListenSocket],
    ports: &[u16],
    skip: impl Fn(&ProcessIdentity) -> bool,
) -> Vec<PortOwner> {
    let mut seen = BTreeSet::new();
    let mut owners = Vec::new();
    for socket in sockets {
        if !ports.contains(&socket.port) || !seen.insert((socket.port, socket.pid)) {
            continue;
        }
        let (started_at_secs, parent_pid) = match inspect::vitals(socket.pid) {
            Presence::Running(vitals) => (vitals.started_at_secs, vitals.parent_pid),
            Presence::Unknown => (None, None),
            Presence::Exited => continue,
        };
        let identity = ProcessIdentity {
            pid: socket.pid,
            started_at_secs,
        };
        if skip(&identity) {
            continue;
        }
        owners.push(PortOwner {
            port: socket.port,
            identity,
            name: socket.process_name.clone(),
            parent_pid,
        });
    }
    owners
}
