//! Process metadata via `sysinfo`: start time, parent, owner and working directory.

use std::collections::HashMap;

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// What the OS disclosed about a running process. `None` fields were withheld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDetails {
    pub started_at_secs: Option<u64>,
    pub parent_pid: Option<u32>,
    pub user_id: Option<u32>,
    pub effective_user_id: Option<u32>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Running(ProcessDetails),
    /// Gone, or a zombie whose sockets are already closed.
    Exited,
}

/// Owns the `sysinfo` process table so later scans can reuse it (CPU sampling needs history).
pub struct ProcessInspector {
    system: System,
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
        }
    }

    /// Inspects each distinct PID once, however many sockets it holds.
    pub fn inspect(&mut self, pids: &[u32]) -> HashMap<u32, ProcessState> {
        let mut distinct: Vec<Pid> = pids.iter().copied().map(Pid::from_u32).collect();
        distinct.sort_unstable();
        // Required, not just cheaper: sysinfo 0.39 drops a process listed twice in one refresh.
        distinct.dedup();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&distinct),
            true,
            ProcessRefreshKind::nothing()
                .with_user(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        distinct
            .into_iter()
            .map(|pid| (pid.as_u32(), self.state_of(pid)))
            .collect()
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
