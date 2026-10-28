//! Spawning, recording and cleaning up owned fixture processes.
//!
//! Ownership rules:
//! - A fixture is only ever touched through the [`Child`] handle that spawned it. That child is
//!   never reaped before cleanup decides it is gone, so its PID cannot be recycled meanwhile.
//! - Before closing its stdin or signalling it, the live start time must equal the recorded one.
//!   A mismatch refuses the action instead of guessing.
//! - Cleanup is bounded: close stdin (the fixture exits on EOF), wait, then `SIGKILL` through the
//!   handle. A fixture whose owner dies without cleanup still exits on stdin EOF, and at the
//!   latest after its `--max-lifetime-secs`.

use std::{
    env,
    io::{self, BufRead, BufReader, Read},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tempfile::TempDir;

use crate::protocol::{Ready, Serve};

const BINARY: &str = "stackscan-fixture";
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const GRACEFUL_TIMEOUT: Duration = Duration::from_secs(3);
const KILL_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Spaces and non-ASCII on purpose: cwd reporting must survive both.
const CWD_PREFIX: &str = "stackscan fixture é-";

/// What to launch: each group is one port shared by its addresses.
#[derive(Debug, Clone)]
pub struct FixtureSpec {
    groups: Vec<Vec<IpAddr>>,
    max_lifetime: Duration,
    executable_name: Option<String>,
    cwd: Option<PathBuf>,
    busy: bool,
    serve: Serve,
    record: bool,
    extra_args: Vec<String>,
}

impl Default for FixtureSpec {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureSpec {
    #[must_use]
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            max_lifetime: Duration::from_secs(120),
            executable_name: None,
            cwd: None,
            busy: false,
            serve: Serve::Banner,
            record: false,
            extra_args: Vec::new(),
        }
    }

    /// Adds one ephemeral port bound on every address given.
    #[must_use]
    pub fn listen(mut self, addresses: &[IpAddr]) -> Self {
        self.groups.push(addresses.to_vec());
        self
    }

    /// Backstop after which the fixture exits by itself.
    #[must_use]
    pub fn max_lifetime(mut self, lifetime: Duration) -> Self {
        self.max_lifetime = lifetime;
        self
    }

    /// Runs a copy of the fixture binary under this file name (placed in the fixture's cwd), for
    /// tests about how process names are reported.
    #[must_use]
    pub fn executable_name(mut self, name: &str) -> Self {
        self.executable_name = Some(name.to_owned());
        self
    }

    /// Runs in this existing directory (owned by the caller, e.g. a temporary repository)
    /// instead of a fresh temporary one.
    #[must_use]
    pub fn cwd(mut self, dir: &Path) -> Self {
        self.cwd = Some(dir.to_path_buf());
        self
    }

    /// Keeps one thread spinning: a controlled CPU workload of about one core.
    #[must_use]
    pub fn busy(mut self) -> Self {
        self.busy = true;
        self
    }

    /// What every connection gets (default: a non-HTTP banner).
    #[must_use]
    pub fn serve(mut self, serve: Serve) -> Self {
        self.serve = serve;
        self
    }

    /// Records what each connection sent; see [`Fixture::received`].
    #[must_use]
    pub fn record(mut self) -> Self {
        self.record = true;
        self
    }

    /// Extra, ignored arguments, so the process has this command line (after `--`).
    #[must_use]
    pub fn args(mut self, args: &[&str]) -> Self {
        self.extra_args = args.iter().map(|&arg| arg.to_owned()).collect();
        self
    }
}

/// The manifest entry for one owned fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureRecord {
    pub pid: u32,
    /// Unix seconds, read from the OS after the fixture reported readiness.
    pub started_at_secs: u64,
    /// Canonical path (e.g. `/private/var/…` rather than `/var/…` on macOS).
    pub cwd: PathBuf,
    /// The binary that was launched (a copy inside `cwd` when renamed).
    pub executable: PathBuf,
    pub endpoints: Vec<SocketAddr>,
    pub launched_at_ms: u64,
    pub cleanup: Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Cleanup {
    Running,
    /// It had already exited; nothing was sent to it.
    Exited,
    /// It exited after its stdin was closed.
    Closed,
    /// It ignored stdin EOF and was killed through its handle.
    Killed,
}

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error(
        "fixture binary not found at {0}; workspace test runs build it (or run `cargo build -p stackscan-fixtures`)"
    )]
    BinaryMissing(PathBuf),
    #[error("could not launch the fixture: {0}")]
    Spawn(#[source] io::Error),
    #[error("fixture did not become ready: {reason}; stderr: {stderr:?}")]
    NotReady { reason: String, stderr: String },
    #[error("could not read the start time of fixture {0}")]
    NoStartTime(u32),
    #[error("refusing to touch PID {pid}: recorded start {expected}s, live start {found:?}")]
    IdentityMismatch {
        pid: u32,
        expected: u64,
        found: Option<u64>,
    },
    #[error("fixture {pid} still running after {timeout:?}")]
    StillRunning { pid: u32, timeout: Duration },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// One running fixture process. Dropping it cleans it up.
pub struct Fixture {
    record: FixtureRecord,
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: Option<std::process::ChildStderr>,
    // Declared last so the directory outlives the process that runs in it. `None` when the
    // caller supplied the directory.
    _cwd: Option<TempDir>,
}

impl Fixture {
    /// Launches a fixture and waits (bounded) until every socket is listening.
    pub fn spawn(spec: &FixtureSpec) -> Result<Self, FixtureError> {
        let (cwd, canonical_cwd) = if let Some(dir) = &spec.cwd {
            (None, dir.canonicalize()?)
        } else {
            let dir = tempfile::Builder::new().prefix(CWD_PREFIX).tempdir()?;
            let canonical = dir.path().canonicalize()?;
            (Some(dir), canonical)
        };
        let binary = match &spec.executable_name {
            Some(name) => {
                let renamed = canonical_cwd.join(name);
                std::fs::copy(binary_path()?, &renamed)?;
                renamed
            }
            None => binary_path()?,
        };

        let mut command = Command::new(&binary);
        for group in &spec.groups {
            let joined: Vec<String> = group.iter().map(ToString::to_string).collect();
            command.arg("--listen").arg(joined.join(","));
        }
        if spec.busy {
            command.arg("--busy");
        }
        if spec.record {
            command.arg("--record");
        }
        command.arg("--serve").arg(spec.serve.as_arg());
        command
            .arg("--max-lifetime-secs")
            .arg(spec.max_lifetime.as_secs().max(1).to_string())
            .arg("--")
            .args(&spec.extra_args)
            .current_dir(&canonical_cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let launched_at_ms = unix_millis(SystemTime::now());
        let mut child = command.spawn().map_err(FixtureError::Spawn)?;
        let stdin = child.stdin.take();
        let stderr = child.stderr.take();
        let mut fixture = Self {
            record: FixtureRecord {
                pid: child.id(),
                started_at_secs: 0,
                cwd: canonical_cwd,
                executable: binary,
                endpoints: Vec::new(),
                launched_at_ms,
                cleanup: Cleanup::Running,
            },
            child,
            stdin,
            stderr,
            _cwd: cwd,
        };

        // On any failure below, `fixture` drops and kills the child through its own handle.
        let ready = fixture.await_ready()?;
        fixture.record.endpoints = ready.listeners;
        fixture.record.started_at_secs =
            live_start_time(fixture.record.pid).ok_or(FixtureError::NoStartTime(ready.pid))?;
        Ok(fixture)
    }

    fn await_ready(&mut self) -> Result<Ready, FixtureError> {
        let stdout = self
            .child
            .stdout
            .take()
            .ok_or_else(|| FixtureError::NotReady {
                reason: "stdout was not captured".into(),
                stderr: String::new(),
            })?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let not_ready = |fixture: &mut Self, reason: String| {
            let _ = fixture.kill_owned_child();
            FixtureError::NotReady {
                reason,
                stderr: fixture.drain_stderr(),
            }
        };
        let line = match receiver.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(line)) if !line.is_empty() => line,
            Ok(Ok(_)) => return Err(not_ready(self, "exited before ready".into())),
            Ok(Err(error)) => return Err(not_ready(self, error.to_string())),
            Err(_) => {
                return Err(not_ready(
                    self,
                    format!("timed out after {READY_TIMEOUT:?}"),
                ));
            }
        };
        let ready: Ready = serde_json::from_str(&line)
            .map_err(|error| not_ready(self, format!("malformed ready line: {error}")))?;
        if ready.pid != self.record.pid {
            return Err(not_ready(
                self,
                format!("ready line names PID {}", ready.pid),
            ));
        }
        Ok(ready)
    }

    #[must_use]
    pub fn record(&self) -> &FixtureRecord {
        &self.record
    }

    #[must_use]
    pub fn pid(&self) -> u32 {
        self.record.pid
    }

    /// What connections to `port` sent so far, one entry per connection, when spawned with
    /// [`FixtureSpec::record`]. Empty if nothing connected.
    #[must_use]
    pub fn received(&self, port: u16) -> Vec<String> {
        let log = self.record.cwd.join(format!("received-{port}.log"));
        std::fs::read(log).map_or_else(
            |_| Vec::new(),
            |bytes| {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let mut entries: Vec<String> = text.split("\n--\n").map(str::to_owned).collect();
                entries.pop(); // after the last separator
                entries
            },
        )
    }

    /// Mutable access for tests that must exercise the identity check with a wrong record.
    pub fn record_mut(&mut self) -> &mut FixtureRecord {
        &mut self.record
    }

    /// Stops the fixture: identity check, close stdin, bounded wait, then `SIGKILL` via the handle.
    pub fn stop(&mut self) -> Result<Cleanup, FixtureError> {
        if self.child.try_wait()?.is_some() {
            return Ok(self.finish(Cleanup::Exited));
        }
        self.verify_identity()?;
        drop(self.stdin.take());
        if self.wait_for_exit(GRACEFUL_TIMEOUT)?.is_some() {
            return Ok(self.finish(Cleanup::Closed));
        }
        self.verify_identity()?;
        self.kill_owned_child()?;
        match self.wait_for_exit(KILL_TIMEOUT)? {
            Some(_) => Ok(self.finish(Cleanup::Killed)),
            None => Err(FixtureError::StillRunning {
                pid: self.record.pid,
                timeout: KILL_TIMEOUT,
            }),
        }
    }

    /// Sends `SIGKILL` (after the identity check) without reaping, leaving a zombie until
    /// [`Fixture::wait`]. Lets tests observe a process that ended but was not yet collected.
    pub fn kill(&mut self) -> Result<(), FixtureError> {
        self.verify_identity()?;
        Ok(self.kill_owned_child()?)
    }

    /// Waits (bounded) for the fixture to exit and reaps it.
    pub fn wait(&mut self, timeout: Duration) -> Result<ExitStatus, FixtureError> {
        match self.wait_for_exit(timeout)? {
            Some(status) => {
                self.finish(Cleanup::Exited);
                Ok(status)
            }
            None => Err(FixtureError::StillRunning {
                pid: self.record.pid,
                timeout,
            }),
        }
    }

    fn verify_identity(&self) -> Result<(), FixtureError> {
        let found = live_start_time(self.record.pid);
        if found == Some(self.record.started_at_secs) {
            Ok(())
        } else {
            Err(FixtureError::IdentityMismatch {
                pid: self.record.pid,
                expected: self.record.started_at_secs,
                found,
            })
        }
    }

    /// Only through our own unreaped handle, so the PID still names our child.
    fn kill_owned_child(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn finish(&mut self, cleanup: Cleanup) -> Cleanup {
        self.record.cleanup = cleanup;
        cleanup
    }

    fn drain_stderr(&mut self) -> String {
        let mut output = String::new();
        if let Some(mut stderr) = self.stderr.take() {
            // Only called after the child was killed, so this reaches EOF.
            let _ = stderr.read_to_string(&mut output);
        }
        output
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.record.cleanup != Cleanup::Running {
            return;
        }
        if self.stop().is_err() {
            // The identity check refused (or the kill timed out). Closing stdin is not a signal:
            // our own fixture exits on EOF, anything else is left alone.
            drop(self.stdin.take());
            let _ = self.wait_for_exit(GRACEFUL_TIMEOUT);
        }
    }
}

/// Start time of a live, non-zombie process, in Unix seconds.
#[must_use]
pub fn live_start_time(pid: u32) -> Option<u64> {
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let process = system.process(pid)?;
    if process.status() == sysinfo::ProcessStatus::Zombie {
        return None;
    }
    Some(process.start_time()).filter(|&secs| secs > 0)
}

/// `stackscan-fixture` next to the running test executable (`target/<profile>/deps/…`).
fn binary_path() -> Result<PathBuf, FixtureError> {
    let exe = env::current_exe()?;
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .ok_or_else(|| FixtureError::BinaryMissing(exe.clone()))?;
    let binary = profile_dir.join(BINARY);
    if binary.is_file() {
        Ok(binary)
    } else {
        Err(FixtureError::BinaryMissing(binary))
    }
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}
