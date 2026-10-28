//! Bounded `git` subprocesses. Git's own discovery handles every `.git` form (directory, gitfile
//! of a linked worktree or submodule, `--separate-git-dir`) and ref storage (files or reftable),
//! so Stackscan never parses `.git` contents itself.
//!
//! `git` is run from the developer directory reported by `xcode-select -p`, never through the
//! `/usr/bin/git` shim: without the Command Line Tools the shim opens an installation dialog,
//! which a background scanner must not trigger.

use std::{
    env,
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::model::AttributionFailure;

/// Generous for commands that take a few milliseconds, short enough that a stalled file system
/// costs one scan a bounded delay (failures are cached; see `attribution`).
pub const GIT_TIMEOUT: Duration = Duration::from_secs(2);
const XCODE_SELECT: &str = "/usr/bin/xcode-select";
const POLL_INTERVAL: Duration = Duration::from_millis(1);
/// Larger output than any command here produces is treated as a failure.
const MAX_OUTPUT: u64 = 64 * 1024;

/// What a finished command reported.
pub struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    TimedOut,
    Failed,
}

impl From<RunError> for AttributionFailure {
    fn from(error: RunError) -> Self {
        match error {
            RunError::TimedOut => Self::GitTimedOut,
            RunError::Failed => Self::GitFailed,
        }
    }
}

/// A located `git` executable plus a count of the commands it ran.
pub struct Git {
    executable: PathBuf,
    commands: u64,
}

impl Git {
    /// `git` from the active developer directory (Xcode or the Command Line Tools), if any.
    #[must_use]
    pub fn locate() -> Option<Self> {
        let mut command = Command::new(XCODE_SELECT);
        command.arg("-p");
        let output = run_bounded(&mut command, GIT_TIMEOUT).ok()?;
        let developer_dir = output.stdout.trim_end_matches('\n');
        if !output.success || developer_dir.is_empty() {
            return None;
        }
        let executable = Path::new(developer_dir).join("usr/bin/git");
        executable.is_file().then_some(Self {
            executable,
            commands: 0,
        })
    }

    /// Runs `git -C <dir> <args>` with discovery confined to `dir` (its parent is the ceiling),
    /// no inherited `GIT_*` overrides, no prompts and no optional locks.
    pub fn run(&mut self, dir: &Path, args: &[&str]) -> Result<Output, RunError> {
        let mut command = Command::new(&self.executable);
        for (key, _) in env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        if let Some(parent) = dir.parent() {
            command.env("GIT_CEILING_DIRECTORIES", parent);
        }
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .args(["--no-pager", "-c", "core.fsmonitor=false", "-C"])
            .arg(dir)
            .args(args.iter().map(OsStr::new));
        self.commands += 1;
        run_bounded(&mut command, GIT_TIMEOUT)
    }

    /// Commands run so far.
    #[must_use]
    pub fn commands(&self) -> u64 {
        self.commands
    }
}

/// Runs a short command to completion within `timeout`, killing it (our own child, through its
/// handle) when it overruns. Stdout is drained on a helper thread while the child runs, so large
/// output cannot block the child into a false timeout, and a grandchild that inherited the pipe
/// cannot hold the read open past the deadline (the helper then finishes on its own once the pipe
/// closes).
fn run_bounded(command: &mut Command, timeout: Duration) -> Result<Output, RunError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| RunError::Failed)?;
    let deadline = Instant::now() + timeout;
    let (sender, receiver) = mpsc::channel();
    if let Some(pipe) = child.stdout.take() {
        thread::spawn(move || {
            let mut stdout = Vec::new();
            let read = pipe
                .take(MAX_OUTPUT + 1)
                .read_to_end(&mut stdout)
                .map(|_| stdout);
            let _ = sender.send(read);
        });
    }
    let mut status = None;
    let mut stdout: Option<Vec<u8>> = None;
    let outcome = loop {
        if stdout.is_none() {
            match receiver.try_recv() {
                Ok(Ok(bytes)) => stdout = Some(bytes),
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => break Err(RunError::Failed),
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if stdout
            .as_ref()
            .is_some_and(|bytes| bytes.len() as u64 > MAX_OUTPUT)
        {
            break Err(RunError::Failed);
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(found) => status = found,
                Err(_) => break Err(RunError::Failed),
            }
        }
        if let (Some(status), Some(bytes)) = (status, stdout.as_ref()) {
            break Ok(Output {
                success: status.success(),
                code: status.code(),
                stdout: String::from_utf8_lossy(bytes).into_owned(),
            });
        }
        if Instant::now() >= deadline {
            break Err(RunError::TimedOut);
        }
        thread::sleep(POLL_INTERVAL);
    };
    if outcome.is_err() && status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn output_is_returned_with_the_exit_code() {
        let output = run_bounded(&mut shell("printf 'a\\nb\\n'; exit 3"), GIT_TIMEOUT)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(!output.success);
        assert_eq!(output.code, Some(3));
        assert_eq!(output.stdout, "a\nb\n");
    }

    #[test]
    fn endless_output_fails_fast_instead_of_timing_out() {
        let started = Instant::now();
        let result = run_bounded(&mut Command::new("/usr/bin/yes"), Duration::from_secs(5));
        assert_eq!(result.err(), Some(RunError::Failed));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn hung_command_is_killed_at_the_deadline() {
        let started = Instant::now();
        let result = run_bounded(&mut shell("exec sleep 5"), Duration::from_millis(200));
        assert_eq!(result.err(), Some(RunError::TimedOut));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn grandchild_holding_stdout_cannot_hold_the_read_past_the_deadline() {
        // The shell exits at once; its background `sleep` keeps the pipe's write end open.
        let started = Instant::now();
        let result = run_bounded(
            &mut shell("sleep 1 & echo started"),
            Duration::from_millis(200),
        );
        assert_eq!(result.err(), Some(RunError::TimedOut));
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "{:?}",
            started.elapsed()
        );
    }
}
