//! Starting the dashboard window (`kosse window`, see [`crate::window`]) as a child process, and
//! falling back to the default browser, with the reason, when it cannot start.
//!
//! The window is owned through its stdin: when the owner exits (even when it is killed) the pipe
//! closes and the window closes too, so a window never outlives its owner. Its exit is reaped by
//! the thread that reads its output, as soon as it happens.

use std::{
    io::{self, BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::opener;

/// How long a closing window may take before its owner kills it (through its own child handle).
const CLOSE_DEADLINE: Duration = Duration::from_secs(2);
/// How long the reader waits for an exited window's process to be reapable.
const REAP_DEADLINE: Duration = Duration::from_secs(5);

/// The app's name in window titles and the tray tooltip.
#[cfg(not(debug_assertions))]
pub const TITLE: &str = "Kossé";
/// Debug builds (`bun run app`, `bun run dev`, tests) say they are a preview, and of which branch,
/// so they are never mistaken for the installed app (with the split-stem K icons, `tray.rs`,
/// `activation.rs`).
#[cfg(debug_assertions)]
pub const TITLE: &str = concat!("Kossé · preview", env!("KOSSE_PREVIEW_SUFFIX"));

/// The window title for a service: `<TITLE> — <service>`.
pub fn title_for(service: &str) -> String {
    format!("{TITLE} — {service}")
}

/// Opens `url` in the default browser because the window is unavailable, and says why.
pub fn fallback(url: &str, reason: &str) {
    report!("kosse: {reason}; opening {url} in the default browser");
    opener::open_or_report(url);
}

/// What the owner learns about a window after the fact (from the reader thread).
pub enum Notice {
    /// It never started; the browser fallback ran.
    Unavailable { reason: String },
    /// It ended with a route it had been asked to show but had not shown yet (closed by the user
    /// at that moment): the owner shows that route again in a new window.
    Lost { url: String, title: String },
}

pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

/// One running window process.
pub struct Window {
    child: Arc<Mutex<Child>>,
    pid: u32,
    stdin: Option<ChildStdin>,
    state: Arc<State>,
    reader: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct State {
    started: AtomicBool,
    exited: AtomicBool,
    /// Sent but not yet acknowledged with `shown`.
    pending: Mutex<Option<(String, String)>>,
}

impl State {
    fn pending(&self) -> std::sync::MutexGuard<'_, Option<(String, String)>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Window {
    /// Starts `kosse window <url>`. If it exits before reporting `ready`, `url` opens in the
    /// default browser instead and the reason is reported (and passed to `notify`).
    pub fn spawn(url: &str, title: &str, notify: Option<Notify>) -> io::Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .args(["window", url, "--title", title])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let pid = child.id();
        let child = Arc::new(Mutex::new(child));
        let state = Arc::new(State::default());
        let reader = match stdout {
            Some(stdout) => {
                let (reader_child, reader_state) = (Arc::clone(&child), Arc::clone(&state));
                let reader_url = url.to_owned();
                let spawned = thread::Builder::new()
                    .name("kosse-window-reader".into())
                    .spawn(move || {
                        follow(
                            BufReader::new(stdout),
                            &reader_child,
                            &reader_state,
                            &reader_url,
                            notify,
                        );
                    });
                match spawned {
                    Ok(reader) => Some(reader),
                    Err(error) => {
                        kill(&child);
                        return Err(error);
                    }
                }
            }
            None => None,
        };
        Ok(Self {
            child,
            pid,
            stdin,
            state,
            reader,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Asks the running window to show `url` and come forward. `false` if it is gone: the caller
    /// then starts a new one.
    pub fn show(&mut self, url: &str, title: &str) -> bool {
        if self.exited() {
            return false;
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return false;
        };
        *self.state.pending() = Some((url.to_owned(), title.to_owned()));
        let sent = writeln!(stdin, "{url}\t{title}").and_then(|()| stdin.flush());
        if sent.is_err() {
            // Closed between the check and the write: the caller starts a new window.
            *self.state.pending() = None;
            return false;
        }
        true
    }

    /// Whether the window process has ended (closed by the user, or failed to start).
    pub fn exited(&self) -> bool {
        if self.state.exited.load(Ordering::Acquire) {
            return true;
        }
        let ended = !matches!(
            self.child
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_wait(),
            Ok(None)
        );
        if ended {
            self.state.exited.store(true, Ordering::Release);
        }
        ended
    }

    /// Whether the window reported `ready`.
    pub fn started(&self) -> bool {
        self.state.started.load(Ordering::Acquire)
    }

    /// Closes the window: end of input asks it to close, then this child is killed if it is still
    /// running after [`CLOSE_DEADLINE`]. Also waits for the fallback, if one is running.
    pub fn close(mut self) {
        // Closing on purpose: nothing it was asked to show is "lost".
        *self.state.pending() = None;
        drop(self.stdin.take());
        let deadline = Instant::now() + CLOSE_DEADLINE;
        while !self.exited() {
            if Instant::now() >= deadline {
                kill(&self.child);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn kill(child: &Mutex<Child>) {
    let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads the window's protocol lines (forwarding anything else, test reports, to stdout), then
/// reaps it and tells the owner what it needs to know.
fn follow(
    stdout: impl BufRead,
    child: &Mutex<Child>,
    state: &State,
    url: &str,
    notify: Option<Notify>,
) {
    let mut failure = None;
    for line in stdout.lines() {
        let Ok(line) = line else { break };
        if line == "ready" {
            state.started.store(true, Ordering::Release);
        } else if let Some(reason) = line.strip_prefix("failed ") {
            failure = Some(reason.to_owned());
        } else if line.starts_with("shown ") {
            *state.pending() = None;
        } else {
            let mut out = io::stdout().lock();
            let _ = writeln!(out, "window: {line}");
            let _ = out.flush();
        }
    }
    let deadline = Instant::now() + REAP_DEADLINE;
    loop {
        let reaped = child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_wait();
        if !matches!(reaped, Ok(None)) || Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    state.exited.store(true, Ordering::Release);
    if !state.started.load(Ordering::Acquire) {
        let reason = format!(
            "the dashboard window did not start ({})",
            failure
                .as_deref()
                .unwrap_or("it exited before showing a window")
        );
        fallback(url, &reason);
        if let Some(notify) = notify {
            notify(Notice::Unavailable { reason });
        }
    } else if let Some((url, title)) = state.pending().take()
        && let Some(notify) = notify
    {
        notify(Notice::Lost { url, title });
    }
}

/// The app's one window: focus-or-spawn, so repeated Open Dashboard / Inspect / Stop… reuse it.
pub struct Windows {
    current: Option<Window>,
    notify: Notify,
}

impl Windows {
    pub fn new(notify: Notify) -> Self {
        Self {
            current: None,
            notify,
        }
    }

    /// Shows `url` in the window. Returns the PID of a window that already ran (the caller brings
    /// it forward); a new window comes forward by itself.
    pub fn show(&mut self, url: &str, title: &str) -> Option<u32> {
        if let Some(window) = self.current.as_mut()
            && window.show(url, title)
        {
            return Some(window.pid());
        }
        if let Some(gone) = self.current.take() {
            gone.close();
        }
        match Window::spawn(url, title, Some(Arc::clone(&self.notify))) {
            Ok(window) => self.current = Some(window),
            Err(error) => {
                let reason = format!("could not start the dashboard window ({error})");
                fallback(url, &reason);
                (self.notify)(Notice::Unavailable { reason });
            }
        }
        None
    }

    pub fn close(&mut self) {
        if let Some(window) = self.current.take() {
            window.close();
        }
    }
}
