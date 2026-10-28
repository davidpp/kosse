//! Process metadata via `sysinfo`: start time, parent, owner, command line, working directory,
//! resident memory and accumulated CPU time. The command line is redacted here, so the raw
//! argument vector never leaves this module. [`vitals`] is the minimal fresh look at one PID
//! (one `sysinfo` refresh of that PID, BSD info only) that actions revalidate with immediately
//! before signalling.

use std::collections::{HashMap, HashSet};

use crate::{model::CommandLine, redact};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// What the OS disclosed about a running process. `None` fields were withheld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDetails {
    pub started_at_secs: Option<u64>,
    pub parent_pid: Option<u32>,
    pub user_id: Option<u32>,
    pub effective_user_id: Option<u32>,
    /// Redacted; `None` when the OS withheld the arguments (other users' processes).
    pub command: Option<CommandLine>,
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
                // A process's arguments are read once; argv rewrites after start are not seen.
                .with_cmd(UpdateKind::OnlyIfNotSet)
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
            command: Some(process.cmd())
                .filter(|args| !args.is_empty())
                .map(redact::command_line),
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

    /// The effective user ID of this process: root (0) sees every user's sockets, and only
    /// this user's processes are probed automatically. `None` when even our own process could not
    /// be inspected.
    pub fn own_effective_user(&mut self) -> Option<u32> {
        let me = sysinfo::get_current_pid().ok()?;
        match self.inspect(&[me.as_u32()]).remove(&me.as_u32()) {
            Some(ProcessState::Running(details)) => details.effective_user_id,
            _ => None,
        }
    }
}

/// Who a PID names right now: the fields actions decide on, read fresh (no retained table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessVitals {
    pub started_at_secs: Option<u64>,
    pub parent_pid: Option<u32>,
    pub user_id: Option<u32>,
    pub effective_user_id: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Running(ProcessVitals),
    /// A zombie (its sockets are closed and it can no longer act), or `kill(pid, 0)` reports
    /// that no such process exists.
    Exited,
    /// The process exists but `sysinfo` returned nothing about it (it drops processes whose
    /// name it cannot read). Never treated as gone.
    Unknown,
}

/// One `sysinfo` refresh of that PID (BSD info only: no cwd, CPU or memory refresh, so the check
/// before a signal stays short; `sysinfo` still reads the argument area to name the process).
#[must_use]
pub fn vitals(pid: u32) -> Presence {
    let sys_pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sys_pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let Some(process) = system.process(sys_pid) else {
        // sysinfo also drops zombies (their name is unreadable) and live processes whose name it
        // cannot read; only the kernel's answer tells them apart.
        return if !exists(pid) || is_zombie(pid) == Some(true) {
            Presence::Exited
        } else {
            Presence::Unknown
        };
    };
    if matches!(
        process.status(),
        ProcessStatus::Zombie | ProcessStatus::Dead
    ) {
        return Presence::Exited;
    }
    Presence::Running(ProcessVitals {
        started_at_secs: Some(process.start_time()).filter(|&secs| secs > 0),
        parent_pid: process.parent().map(Pid::as_u32),
        user_id: process.user_id().map(|uid| **uid),
        effective_user_id: process.effective_user_id().map(|uid| **uid),
    })
}

/// `kill(pid, 0)`: sends nothing. Only `ESRCH` means the PID is free; `EPERM` (another user's
/// process) and anything unexpected count as existing.
fn exists(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return true;
    };
    !matches!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    )
}

/// `ps -o stat= -p <pid>` (absolute path, fixed arguments, no shell); `None` if it gave no answer.
/// Only used when sysinfo returned nothing for a PID that exists.
fn is_zombie(pid: u32) -> Option<bool> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let stat = String::from_utf8_lossy(&output.stdout);
    let stat = stat.trim();
    (output.status.success() && !stat.is_empty()).then(|| stat.starts_with('Z'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_free_pid_reads_as_exited() {
        assert!(matches!(
            vitals(std::process::id()),
            Presence::Running(ProcessVitals {
                started_at_secs: Some(_),
                ..
            })
        ));
        // launchd: macOS discloses nothing about it to a user, yet it is never "gone".
        assert_ne!(vitals(1), Presence::Exited);
        let free = (90_000..99_999)
            .rev()
            .find(|&pid| !exists(pid))
            .expect("a free PID");
        assert_eq!(vitals(free), Presence::Exited);
    }
}
