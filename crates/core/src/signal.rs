//! The only place Kossé sends a signal: `kill(2)` to one positive PID that passed
//! [`crate::action::revalidate`]. Never a process group (`kill(-pgid)`), never `kill(-1)`.
//!
//! A PID plus start time is not a kernel handle: macOS has no pidfd, so the process can exit and
//! its PID be reused between the revalidation and this call. Callers keep that interval to an
//! in-memory check; see blueprint/architecture.md#actions.

use std::io;

use rustix::process::{Pid, Signal as OsSignal, kill_process};

use crate::action::Eligible;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `SIGTERM`: ask the process to stop.
    Terminate,
    /// `SIGKILL`: cannot be caught or ignored.
    Kill,
}

#[derive(Debug, thiserror::Error)]
pub enum SignalError {
    /// The process exited (and was reaped) before the signal.
    #[error("the process no longer exists")]
    NoSuchProcess,
    #[error("the OS refused the signal")]
    PermissionDenied,
    #[error("could not send the signal: {0}")]
    Os(#[source] io::Error),
}

pub fn send(target: &Eligible, signal: Signal) -> Result<(), SignalError> {
    // `Eligible` PIDs are above 1; `try_from` additionally rules out a negative (group) PID.
    let pid = i32::try_from(target.pid())
        .ok()
        .filter(|&raw| raw > 1)
        .and_then(Pid::from_raw)
        .ok_or_else(|| SignalError::Os(io::Error::from(io::ErrorKind::InvalidInput)))?;
    let signal = match signal {
        Signal::Terminate => OsSignal::TERM,
        Signal::Kill => OsSignal::KILL,
    };
    kill_process(pid, signal).map_err(|errno| match errno {
        rustix::io::Errno::SRCH => SignalError::NoSuchProcess,
        rustix::io::Errno::PERM => SignalError::PermissionDenied,
        other => SignalError::Os(other.into()),
    })
}
