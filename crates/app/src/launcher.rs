//! Starting the dashboard window (`kosse window`, see [`crate::native`]) as a child process, and
//! reporting a native startup failure to the owner when it cannot start.
//!
//! The window is owned through its stdin: when the owner exits (even when it is killed) the pipe
//! closes and the window closes too, so a window never outlives its owner. Routes travel on that
//! pipe as one JSON [`Show`] per line, the first one right after the spawn. Its exit is reaped by
//! the thread that reads its output, as soon as it happens, and reported as [`Notice::Exited`].

use crate::route::{Route, Show};
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

/// How long a closing window may take before its owner kills it (through its own child handle).
const CLOSE_DEADLINE: Duration = Duration::from_secs(2);
/// How long the reader waits for an exited window's process to be reapable.
const REAP_DEADLINE: Duration = Duration::from_secs(5);

/// The app's name in window titles and the tray tooltip.
#[cfg(not(debug_assertions))]
pub const TITLE: &str = "Kossé";
/// Debug builds (`bun run app`, `bun run dev`, tests) say they are a preview, and of which branch,
/// so they are never mistaken for the installed app (with the split-stem preview icons, `tray.rs`,
/// `activation.rs`).
#[cfg(debug_assertions)]
pub const TITLE: &str = concat!("Kossé · preview", env!("KOSSE_PREVIEW_SUFFIX"));

/// The window title for a service: `<TITLE> — <service>`.
pub fn title_for(service: &str) -> String {
    format!("{TITLE} — {service}")
}

/// Report a native window failure without launching an unrelated application.
pub fn fallback(url: &str, reason: &str) {
    report!("kosse: {reason}; native window unavailable for {url}. Retry with `kosse open`.");
}

/// What the owner learns about a window after the fact (from the reader thread).
pub enum Notice {
    /// It never started; the failure was reported.
    Unavailable { reason: String },
    /// It ended with a route it had been asked to show but had not shown yet (closed by the user
    /// at that moment): the owner shows that route again in a new window.
    Lost { route: Route, title: String },
    /// Its process ended (after any other notice about it).
    Exited,
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
    pending: Mutex<Option<(Route, String)>>,
}

impl State {
    fn pending(&self) -> std::sync::MutexGuard<'_, Option<(Route, String)>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Window {
    /// Starts `kosse window <url>` (the instance's base URL) showing `route`. Failure before
    /// `ready` is reported and passed to `notify`.
    pub fn spawn(url: &str, route: Route, title: &str, notify: Option<Notify>) -> io::Result<Self> {
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
        let mut window = Self {
            child,
            pid,
            stdin,
            state,
            reader,
        };
        // A window that cannot take its first route has exited; the reader reports that.
        let _ = window.show(route, title);
        Ok(window)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Asks the running window to show `route` and come forward. `false` if it is gone: the
    /// caller then starts a new one.
    pub fn show(&mut self, route: Route, title: &str) -> bool {
        if self.exited() {
            return false;
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return false;
        };
        let show = Show {
            route: route.into(),
            title: title.to_owned(),
        };
        let Ok(line) = serde_json::to_string(&show) else {
            return false;
        };
        *self.state.pending() = Some((route, show.title));
        let sent = writeln!(stdin, "{line}").and_then(|()| stdin.flush());
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
    /// running after [`CLOSE_DEADLINE`]. Also joins the output reader.
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
            notify(Notice::Exited);
        }
        return;
    }
    let lost = state.pending().take();
    if let Some(notify) = notify {
        if let Some((route, title)) = lost {
            notify(Notice::Lost { route, title });
        }
        notify(Notice::Exited);
    }
}

/// The app's one window: focus-or-spawn, so repeated Open Dashboard / Inspect / Stop… reuse it.
pub struct Windows {
    /// The instance's base URL, `http://127.0.0.1:<port>/`.
    url: String,
    current: Option<Window>,
    notify: Notify,
}

impl Windows {
    pub fn new(url: &str, notify: Notify) -> Self {
        Self {
            url: url.to_owned(),
            current: None,
            notify,
        }
    }

    /// Shows `route` in the window. Returns the PID of a window that already ran (the caller
    /// brings it forward); a new window comes forward by itself.
    pub fn show(&mut self, route: Route, title: &str) -> Option<u32> {
        if let Some(window) = self.current.as_mut()
            && window.show(route, title)
        {
            return Some(window.pid());
        }
        if let Some(gone) = self.current.take() {
            gone.close();
        }
        match Window::spawn(&self.url, route, title, Some(Arc::clone(&self.notify))) {
            Ok(window) => self.current = Some(window),
            Err(error) => {
                let reason = format!("could not start the dashboard window ({error})");
                fallback(&self.url, &reason);
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
