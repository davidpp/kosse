//! Process metadata via `sysinfo`: start time, parent, owner, working directory, resident memory
//! and accumulated CPU time.

use std::collections::{HashMap, HashSet};

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// What the OS disclosed about a running process. `None` fields were withheld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDetails {
    pub started_at_secs: Option<u64>,
    pub parent_pid: Option<u32>,
    pub user_id: Option<u32>,
    pub effective_user_id: Option<u32>,
    pub cwd: Option<String>,
    /// Resident memory and total CPU time so far, both from one task-info read; `None` when
    /// the OS withheld it (it does for other users' processes).
    pub usage: Option<ResourceUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceUsage {
    pub memory_bytes: u64,
    /// User plus system CPU time since the process started, in milliseconds.
    pub cpu_time_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Running(ProcessDetails),
    /// Gone, or a zombie whose sockets are already closed.
    Exited,
}

/// Owns a `sysinfo` process table. CPU history is kept by the caller (see `metrics`), not by
/// `sysinfo`, so the table can be rebuilt whenever it holds processes that no longer listen.
pub struct ProcessInspector {
    system: System,
    inspected: u64,
}

impl Default for ProcessInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessInspector {
    #[must_use]
    pub fn new() -> Self {
        Self {
            system: System::new(),
            inspected: 0,
        }
    }

    /// Inspects each distinct PID once, however many sockets it holds, in one `sysinfo` refresh.
    pub fn inspect(&mut self, pids: &[u32]) -> HashMap<u32, ProcessState> {
        let mut distinct: Vec<Pid> = pids.iter().copied().map(Pid::from_u32).collect();
        distinct.sort_unstable();
        // Required, not just cheaper: sysinfo 0.39 drops a process listed twice in one refresh.
        distinct.dedup();
        self.inspected += distinct.len() as u64;
        self.prune(&distinct);
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&distinct),
            true,
            ProcessRefreshKind::nothing()
                .with_user(UpdateKind::Always)
                .with_cwd(UpdateKind::Always)
                .with_cpu()
                .with_memory(),
        );
        distinct
            .into_iter()
            .map(|pid| (pid.as_u32(), self.state_of(pid)))
            .collect()
    }

    /// Process inspections so far, counting each PID once per [`ProcessInspector::inspect`].
    #[must_use]
    pub fn inspected(&self) -> u64 {
        self.inspected
    }

    /// A refresh of selected PIDs never drops entries it was not asked about, so a process
    /// that stopped listening (or exited unseen) would stay in the table for good. Rebuilding
    /// before the refresh keeps the table to exactly the processes of the latest scan; nothing
    /// else depends on its history.
    fn prune(&mut self, current: &[Pid]) {
        let current: HashSet<&Pid> = current.iter().collect();
        if self
            .system
            .processes()
            .keys()
            .any(|pid| !current.contains(pid))
        {
            self.system = System::new();
        }
    }

    /// Entries in the retained `sysinfo` table.
    #[must_use]
    pub fn retained(&self) -> usize {
        self.system.processes().len()
    }

    fn state_of(&self, pid: Pid) -> ProcessState {
        let Some(process) = self.system.process(pid) else {
            return ProcessState::Exited;
        };
        if matches!(
            process.status(),
            ProcessStatus::Zombie | ProcessStatus::Dead
        ) {
            return ProcessState::Exited;
        }
        ProcessState::Running(ProcessDetails {
            // sysinfo reports 0 when the kernel refused the BSD info that carries the start time.
            started_at_secs: Some(process.start_time()).filter(|&secs| secs > 0),
            parent_pid: process.parent().map(Pid::as_u32),
            user_id: process.user_id().map(|uid| **uid),
            effective_user_id: process.effective_user_id().map(|uid| **uid),
            cwd: process
                .cwd()
                .map(|cwd| cwd.to_string_lossy().into_owned())
                .filter(|cwd| !cwd.is_empty()),
            // A failed task-info read leaves every field zero; a live process always has
            // resident pages.
            usage: Some(process.memory())
                .filter(|&bytes| bytes > 0)
                .map(|memory_bytes| ResourceUsage {
                    memory_bytes,
                    cpu_time_ms: process.accumulated_cpu_time(),
                }),
        })
    }

    /// Whether this process runs with root privileges, and so can see every user's sockets.
    /// `None` when even our own process could not be inspected.
    pub fn running_as_root(&mut self) -> Option<bool> {
        let me = sysinfo::get_current_pid().ok()?;
        match self.inspect(&[me.as_u32()]).remove(&me.as_u32()) {
            Some(ProcessState::Running(details)) => details.effective_user_id.map(|uid| uid == 0),
            _ => None,
        }
    }
}
