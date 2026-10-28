//! One scan: enumerate listening sockets, inspect each owning process once, assemble a
//! [`Snapshot`]. Gaps (hidden sockets, exited processes, withheld metadata) become warnings.
//!
//! The two phases are not atomic. A PID whose socket was seen can exit, or exit and be reused by
//! a new process, before inspection. A process that started after enumeration began cannot own
//! the socket, so it is reported as the original owner having exited. Start times have
//! one-second resolution: a reuse within the same second as enumeration is not detected.

use std::{
    collections::BTreeMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    endpoints::endpoints_of,
    inspect::{ProcessDetails, ProcessInspector, ProcessState},
    model::{
        ProcessField, ProcessIdentity, ProcessRecord, SNAPSHOT_SCHEMA_VERSION, ScanWarning,
        Service, Snapshot,
    },
    sockets::{ListenSocket, SocketsError, tcp_listen_sockets},
};

/// The application's only scanner. It keeps process state between scans.
pub struct Scanner {
    inspector: ProcessInspector,
    running_as_root: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    #[must_use]
    pub fn new() -> Self {
        let mut inspector = ProcessInspector::new();
        // Unknown privileges are treated as unprivileged: the warning then errs on the side of
        // saying listeners may be missing.
        let running_as_root = inspector.running_as_root().unwrap_or(false);
        Self {
            inspector,
            running_as_root,
        }
    }

    /// Scans the machine now. Fails only when sockets cannot be enumerated at all; anything less
    /// is a snapshot with warnings, never an empty success.
    pub fn scan(&mut self) -> Result<Snapshot, SocketsError> {
        let started = Instant::now();
        let scanned_at = SystemTime::now();
        let sockets = tcp_listen_sockets()?;
        Ok(self.assemble(sockets, started, scanned_at))
    }

    /// The inspection half of [`Scanner::scan`], for sockets enumerated at `enumerated_at`. Lets
    /// tests end (or appear to replace) a process between the two phases.
    pub fn snapshot_of(
        &mut self,
        sockets: Vec<ListenSocket>,
        enumerated_at: SystemTime,
    ) -> Snapshot {
        self.assemble(sockets, Instant::now(), enumerated_at)
    }

    fn assemble(
        &mut self,
        sockets: Vec<ListenSocket>,
        started: Instant,
        scanned_at: SystemTime,
    ) -> Snapshot {
        let mut by_pid: BTreeMap<u32, Vec<ListenSocket>> = BTreeMap::new();
        for socket in sockets {
            by_pid.entry(socket.pid).or_default().push(socket);
        }
        let pids: Vec<u32> = by_pid.keys().copied().collect();
        let mut states = self.inspector.inspect(&pids);
        let enumerated_at_secs = unix_millis(scanned_at) / 1000;

        let mut services = Vec::with_capacity(by_pid.len());
        let mut warnings = Vec::new();
        if !self.running_as_root {
            warnings.push(ScanWarning::OtherUsersNotVisible);
        }
        for (pid, sockets) in by_pid {
            let endpoints = endpoints_of(&sockets);
            match states.remove(&pid) {
                Some(ProcessState::Running(details))
                    if details
                        .started_at_secs
                        .is_none_or(|started| started <= enumerated_at_secs) =>
                {
                    let process = record(pid, &sockets, details);
                    let missing = missing_fields(&process);
                    if !missing.is_empty() {
                        warnings.push(ScanWarning::MetadataUnavailable {
                            pid,
                            fields: missing,
                        });
                    }
                    services.push(Service { process, endpoints });
                }
                // Gone, a zombie, or a newer process now holding the recycled PID.
                Some(ProcessState::Running(_) | ProcessState::Exited) | None => {
                    warnings.push(ScanWarning::ProcessExited {
                        pid,
                        ports: endpoints.iter().map(|endpoint| endpoint.port).collect(),
                    });
                }
            }
        }
        services.sort_by_key(|service| {
            let lowest_port = service.endpoints.first().map(|endpoint| endpoint.port);
            (lowest_port, service.process.identity.pid)
        });

        Snapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            scanned_at_ms: unix_millis(scanned_at),
            scan_duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            services,
            warnings,
        }
    }
}

fn record(pid: u32, sockets: &[ListenSocket], details: ProcessDetails) -> ProcessRecord {
    // Every socket of a PID came from the same libproc pass, so any one names the process.
    let first = sockets.first();
    ProcessRecord {
        identity: ProcessIdentity {
            pid,
            started_at_secs: details.started_at_secs,
        },
        name: first.and_then(|socket| socket.process_name.clone()),
        executable: first.and_then(|socket| socket.executable.clone()),
        cwd: details.cwd,
        parent_pid: details.parent_pid,
        user_id: details.user_id,
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
        (process.cwd.is_none(), ProcessField::Cwd),
    ]
    .into_iter()
    .filter_map(|(missing, field)| missing.then_some(field))
    .collect()
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}
