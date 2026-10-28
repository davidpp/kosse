//! One scan: enumerate listening sockets, inspect each owning process once, attribute its
//! working directory to a project, sample its CPU use, assemble an [`Observation`]. Gaps (hidden
//! sockets, exited processes, withheld metadata, unresolvable projects) become warnings.
//!
//! The two phases are not atomic. A PID whose socket was seen can exit, or exit and be reused by
//! a new process, before inspection. A process that started after enumeration began cannot own
//! the socket, so it is reported as the original owner having exited. Start times have
//! one-second resolution: a reuse within the same second as enumeration is not detected.

use std::{
    collections::{BTreeMap, HashMap},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    attribution::{Attribution, Attributor},
    endpoints::endpoints_of,
    inspect::{ProcessDetails, ProcessInspector, ProcessState},
    metrics::CpuSampler,
    model::{
        CpuUsage, Observation, ProcessField, ProcessIdentity, ProcessMetrics, ProcessRecord,
        Project, ScanWarning, Service,
    },
    probe,
    sockets::{ListenSocket, SocketsError, tcp_listen_sockets},
};

/// The application's only scanner. It keeps process, CPU and attribution state between scans.
pub struct Scanner {
    inspector: ProcessInspector,
    attributor: Attributor,
    cpu: CpuSampler,
    /// Our effective user; `None` (unknown) probes nothing automatically.
    own_user: Option<u32>,
}

/// Cumulative work counters, for checking that scans do not repeat work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanStats {
    /// Process inspections, each distinct PID counted once per scan.
    pub processes_inspected: u64,
    /// `git` subprocesses started for attribution.
    pub git_commands: u64,
    /// Entries in the retained process table.
    pub retained_processes: usize,
    /// Cached attribution entries: working directories and worktrees.
    pub cached_places: usize,
    pub cached_worktrees: usize,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    /// A scanner whose project walks stop at the current user's home directory.
    #[must_use]
    pub fn new() -> Self {
        Self::with_attributor(Attributor::for_current_user())
    }

    #[must_use]
    pub fn with_attributor(attributor: Attributor) -> Self {
        let mut inspector = ProcessInspector::new();
        let own_user = inspector.own_effective_user();
        Self {
            inspector,
            attributor,
            cpu: CpuSampler::default(),
            own_user,
        }
    }

    /// Scans the machine now. Fails only when sockets cannot be enumerated at all; anything less
    /// is an observation with warnings, never an empty success.
    pub fn scan(&mut self) -> Result<Observation, SocketsError> {
        let started = Instant::now();
        let scanned_at = SystemTime::now();
        let sockets = tcp_listen_sockets()?;
        Ok(self.assemble(sockets, started, scanned_at))
    }

    /// The inspection half of [`Scanner::scan`], for sockets enumerated at `enumerated_at`. Lets
    /// tests end (or appear to replace) a process between the two phases.
    pub fn observation_of(
        &mut self,
        sockets: Vec<ListenSocket>,
        enumerated_at: SystemTime,
    ) -> Observation {
        self.assemble(sockets, Instant::now(), enumerated_at)
    }

    #[must_use]
    pub fn stats(&self) -> ScanStats {
        let (cached_places, cached_worktrees) = self.attributor.cached();
        ScanStats {
            processes_inspected: self.inspector.inspected(),
            git_commands: self.attributor.git_commands(),
            retained_processes: self.inspector.retained(),
            cached_places,
            cached_worktrees,
        }
    }

    fn assemble(
        &mut self,
        sockets: Vec<ListenSocket>,
        started: Instant,
        scanned_at: SystemTime,
    ) -> Observation {
        let mut by_pid: BTreeMap<u32, Vec<ListenSocket>> = BTreeMap::new();
        for socket in sockets {
            by_pid.entry(socket.pid).or_default().push(socket);
        }
        let pids: Vec<u32> = by_pid.keys().copied().collect();
        let inspected_at = Instant::now();
        let mut states = self.inspector.inspect(&pids);
        let scanned_at_ms = unix_millis(scanned_at);
        let enumerated_at_secs = scanned_at_ms / 1000;

        let mut warnings = Vec::new();
        // Unknown privileges are treated as unprivileged: the warning then errs on the side of
        // saying listeners may be missing.
        if self.own_user != Some(0) {
            warnings.push(ScanWarning::OtherUsersNotVisible);
        }
        let live = live_owners(by_pid, &mut states, enumerated_at_secs, &mut warnings);

        let readings: Vec<(ProcessIdentity, Option<u64>)> = live
            .iter()
            .map(|(pid, _, details)| {
                (
                    identity(*pid, details),
                    details.usage.map(|usage| usage.cpu_time_ms),
                )
            })
            .collect();
        let cpu = self.cpu.sample(&readings, inspected_at);
        let attributions = self.attributor.attribute(
            live.iter()
                .filter_map(|(_, _, details)| details.cwd.as_deref()),
            Instant::now(),
        );

        let mut projects: BTreeMap<String, Project> = BTreeMap::new();
        let mut services = Vec::with_capacity(live.len());
        for (pid, sockets, details) in &live {
            let identity = identity(*pid, details);
            let process = record(
                identity,
                sockets,
                details,
                cpu.get(&identity).copied().unwrap_or(CpuUsage::Unavailable),
                enumerated_at_secs,
            );
            let missing = missing_fields(&process);
            if !missing.is_empty() {
                warnings.push(ScanWarning::MetadataUnavailable {
                    pid: *pid,
                    fields: missing,
                });
            }
            let attribution = match details.cwd.as_deref().and_then(|cwd| attributions.get(cwd)) {
                Some(Ok(attribution)) => attribution.clone(),
                Some(Err(reason)) => {
                    warnings.push(ScanWarning::AttributionUnavailable {
                        pid: *pid,
                        reason: *reason,
                    });
                    None
                }
                None => None,
            };
            let (project, package) = match attribution {
                Some(Attribution { project, package }) => {
                    let root = project.root.clone();
                    projects.entry(root.clone()).or_insert(project);
                    (Some(root), package)
                }
                None => (None, None),
            };
            let own_user = process.user_id.is_some() && process.user_id == self.own_user;
            let mut endpoints = endpoints_of(sockets);
            for endpoint in &mut endpoints {
                endpoint.http = probe::restrict(endpoint.http.clone(), own_user, project.is_some());
            }
            services.push(Service {
                process,
                endpoints,
                project,
                package,
            });
        }
        services.sort_by_key(|service| {
            let lowest_port = service.endpoints.first().map(|endpoint| endpoint.port);
            (lowest_port, service.process.identity.pid)
        });

        Observation {
            scanned_at_ms,
            scan_duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            services,
            projects: projects.into_values().collect(),
            warnings,
        }
    }
}

/// The processes that still own their sockets; the rest are reported as exited.
fn live_owners(
    by_pid: BTreeMap<u32, Vec<ListenSocket>>,
    states: &mut HashMap<u32, ProcessState>,
    enumerated_at_secs: u64,
    warnings: &mut Vec<ScanWarning>,
) -> Vec<(u32, Vec<ListenSocket>, ProcessDetails)> {
    let mut live = Vec::with_capacity(by_pid.len());
    for (pid, sockets) in by_pid {
        match states.remove(&pid) {
            Some(ProcessState::Running(details))
                if details
                    .started_at_secs
                    .is_none_or(|started| started <= enumerated_at_secs) =>
            {
                live.push((pid, sockets, details));
            }
            // Gone, a zombie, or a newer process now holding the recycled PID.
            Some(ProcessState::Running(_) | ProcessState::Exited) | None => {
                warnings.push(ScanWarning::ProcessExited {
                    pid,
                    ports: endpoints_of(&sockets)
                        .iter()
                        .map(|endpoint| endpoint.port)
                        .collect(),
                });
            }
        }
    }
    live
}

fn identity(pid: u32, details: &ProcessDetails) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        started_at_secs: details.started_at_secs,
    }
}

fn record(
    identity: ProcessIdentity,
    sockets: &[ListenSocket],
    details: &ProcessDetails,
    cpu: CpuUsage,
    scanned_at_secs: u64,
) -> ProcessRecord {
    // Every socket of a PID came from the same libproc pass, so any one names the process.
    let first = sockets.first();
    ProcessRecord {
        identity,
        name: first.and_then(|socket| socket.process_name.clone()),
        executable: first.and_then(|socket| socket.executable.clone()),
        command: details.command.clone(),
        cwd: details.cwd.clone(),
        parent_pid: details.parent_pid,
        user_id: details.user_id,
        age_secs: details
            .started_at_secs
            .map(|started| scanned_at_secs.saturating_sub(started)),
        metrics: ProcessMetrics {
            cpu,
            memory_bytes: details.usage.map(|usage| usage.memory_bytes),
        },
    }
}

fn missing_fields(process: &ProcessRecord) -> Vec<ProcessField> {
    [
        (process.name.is_none(), ProcessField::Name),
        (
            process.identity.started_at_secs.is_none(),
            ProcessField::StartTime,
        ),
        (process.user_id.is_none(), ProcessField::User),
        (process.command.is_none(), ProcessField::Command),
        (process.cwd.is_none(), ProcessField::Cwd),
        (
            process.metrics.cpu == CpuUsage::Unavailable,
            ProcessField::Cpu,
        ),
        (process.metrics.memory_bytes.is_none(), ProcessField::Memory),
    ]
    .into_iter()
    .filter_map(|(missing, field)| missing.then_some(field))
    .collect()
}

/// Wall-clock milliseconds since the Unix epoch (0 for a clock set before 1970).
#[must_use]
pub fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}
