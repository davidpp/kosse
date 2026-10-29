//! `kosse startup enable|disable|status`: opt-in login startup as a per-user `LaunchAgent`.
//!
//! The agent (`~/Library/LaunchAgents/com.davidpp.kosse.plist`) runs the binary that enabled it as
//! the menu-bar app without opening the window (`kosse --no-open`), once per GUI login (`Aqua`
//! sessions only, so an SSH login never starts a tray). It does not keep Kossé alive: Quit stays
//! quit until the next login. Nothing here runs unless the user runs `enable` or `disable`;
//! `status` (and `kosse doctor`) only read.
//!
//! The plist is written here rather than through `auto-launch` or `service-manager`: neither lets
//! tests redirect the agents directory, `service-manager` drives the legacy `launchctl
//! load`/`unload` and `auto-launch` never loads the agent at all. `launchctl bootstrap`/`bootout`
//! on the `gui/<uid>` domain is the supported interface (launchctl(1)).

use std::{
    ffi::OsStr,
    fmt::Write as _,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::Duration,
};

use crate::cli::StartupCommand;

pub const LABEL: &str = "com.davidpp.kosse";
const LAUNCHCTL: &str = "/bin/launchctl";
const PLUTIL: &str = "/usr/bin/plutil";
/// What the agent passes after the program: the menu-bar app, no window at login.
const AGENT_ARGS: [&str; 1] = ["--no-open"];
/// `launchctl`'s exit code for `Input/output error`.
const EIO_EXIT: i32 = 5;
const BOOTSTRAP_RETRIES: u32 = 3;
const BOOTSTRAP_RETRY_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("no home directory to find ~/Library/LaunchAgents in")]
    NoHome,
    #[error("could not locate this executable: {0}")]
    Executable(#[source] io::Error),
    #[error("this executable's path is not UTF-8, which a launch agent cannot name: {0}")]
    NonUtf8Path(PathBuf),
    #[error("could not write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
    #[error("could not remove {}: {source}", path.display())]
    Remove { path: PathBuf, source: io::Error },
    #[error("could not run {program}: {source}")]
    Spawn { program: String, source: io::Error },
    #[error("launchctl {action} failed ({status}): {detail}")]
    Launchctl {
        action: &'static str,
        status: String,
        /// The exit code, to tell launchd's transient `5: Input/output error` apart.
        code: Option<i32>,
        detail: String,
    },
    #[error(
        "{0}; the previous agent was unloaded and its plist replaced, so startup is now off: run `kosse startup enable` again"
    )]
    PreviousUnloaded(#[source] Box<StartupError>),
    #[error("could not write the output: {0}")]
    Output(#[source] io::Error),
}

/// What is configured and what launchd has loaded.
#[derive(Debug)]
pub struct Status {
    pub plist: PathBuf,
    /// The agent's program, when the plist exists; `Err` with the reason it could not be read.
    pub program: Option<Result<PathBuf, String>>,
    pub loaded: Loaded,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    No,
    /// Loaded; running with this PID, or waiting (`None`).
    Yes {
        pid: Option<u32>,
    },
}

impl Status {
    pub fn enabled(&self) -> bool {
        self.program.is_some()
    }

    /// One line for `startup status` and `doctor`.
    pub fn describe(&self) -> String {
        let loaded = match self.loaded {
            Loaded::No => "not loaded",
            Loaded::Yes { pid: Some(_) } => "loaded",
            Loaded::Yes { pid: None } => "loaded, not running",
        };
        let running = match self.loaded {
            Loaded::Yes { pid: Some(pid) } => format!(", running as PID {pid}"),
            _ => String::new(),
        };
        match &self.program {
            None if self.loaded == Loaded::No => "disabled".to_owned(),
            None => format!(
                "disabled, but launchd still has {LABEL} {loaded}{running}; run `kosse startup disable`"
            ),
            Some(Ok(program)) if !program.is_file() => format!(
                "enabled ({}), but its program {} is missing; run `kosse startup enable` from the installed kosse, or `kosse startup disable`",
                self.plist.display(),
                program.display()
            ),
            Some(Ok(program)) => format!(
                "enabled at login: {} {} ({loaded}{running}; {})",
                program.display(),
                AGENT_ARGS.join(" "),
                self.plist.display()
            ),
            Some(Err(reason)) => format!(
                "{} exists but could not be read ({reason}); run `kosse startup disable`",
                self.plist.display()
            ),
        }
    }
}

pub fn run(command: &StartupCommand) -> Result<(), StartupError> {
    let message = match command {
        StartupCommand::Enable => enable()?,
        StartupCommand::Disable => disable()?,
        StartupCommand::Status => status()?.describe(),
    };
    let mut stdout = io::stdout().lock();
    match writeln!(stdout, "{message}").and_then(|()| stdout.flush()) {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => Err(StartupError::Output(error)),
        _ => Ok(()),
    }
}

pub fn status() -> Result<Status, StartupError> {
    let plist = plist_path()?;
    let program = match fs::symlink_metadata(&plist) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => Some(Err(error.to_string())),
        Ok(_) => Some(read_program(&plist)),
    };
    Ok(Status {
        plist,
        program,
        loaded: loaded()?,
    })
}

fn enable() -> Result<String, StartupError> {
    let executable = std::env::current_exe().map_err(StartupError::Executable)?;
    let program = executable
        .to_str()
        .ok_or_else(|| StartupError::NonUtf8Path(executable.clone()))?;
    let plist = plist_path()?;
    let contents = render_plist(program);
    let current = status()?;
    if current.loaded != Loaded::No
        && fs::read_to_string(&plist).is_ok_and(|existing| existing == contents)
    {
        return Ok(format!("startup already enabled: {}", current.describe()));
    }
    let replacing = current.loaded != Loaded::No;
    if replacing {
        bootout()?;
    }
    write_atomically(&plist, &contents)?;
    if let Err(error) = bootstrap(&plist) {
        // Leave nothing half-enabled: no agent that would start at the next login only.
        let _ = fs::remove_file(&plist);
        return Err(if replacing {
            StartupError::PreviousUnloaded(Box::new(error))
        } else {
            error
        });
    }
    let mut message = format!(
        "startup enabled: {program} {} runs at each login ({}); it was also started now",
        AGENT_ARGS.join(" "),
        plist.display()
    );
    if in_build_directory(&executable) {
        message.push_str(
            "\nnote: this kosse is in a build directory; enable startup from the installed kosse so a rebuild or `cargo clean` does not break it",
        );
    }
    Ok(message)
}

fn disable() -> Result<String, StartupError> {
    let current = status()?;
    if current.loaded != Loaded::No {
        // Stops a Kossé that launchd started (Quit semantics: observed services keep running).
        bootout()?;
    }
    if current.enabled() {
        fs::remove_file(&current.plist).map_err(|source| StartupError::Remove {
            path: current.plist.clone(),
            source,
        })?;
    }
    Ok(match (current.enabled(), current.loaded) {
        (false, Loaded::No) => "startup was not enabled".to_owned(),
        _ => format!("startup disabled (removed {})", current.plist.display()),
    })
}

/// The launch agent this build writes. Every value is XML-escaped.
fn render_plist(program: &str) -> String {
    let mut arguments = String::new();
    for argument in std::iter::once(program).chain(AGENT_ARGS) {
        let _ = writeln!(arguments, "\t\t<string>{}</string>", escape(argument));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
{arguments}	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
	<key>ProcessType</key>
	<string>Interactive</string>
</dict>
</plist>
"#
    )
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn plist_path() -> Result<PathBuf, StartupError> {
    let home = std::env::home_dir().ok_or(StartupError::NoHome)?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

/// The agent's program through `plutil`, so a hand-edited plist (any plist format) still reads.
fn read_program(plist: &Path) -> Result<PathBuf, String> {
    let output = Command::new(PLUTIL)
        .args(["-extract", "ProgramArguments.0", "raw", "-o", "-"])
        .arg(plist)
        .output()
        .map_err(|error| format!("could not run {PLUTIL}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "no ProgramArguments: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let program = String::from_utf8_lossy(&output.stdout);
    Ok(PathBuf::from(program.trim_end_matches('\n')))
}

fn domain() -> String {
    format!("gui/{}", rustix::process::getuid().as_raw())
}

fn loaded() -> Result<Loaded, StartupError> {
    let target = format!("{}/{LABEL}", domain());
    let output = run_launchctl(&[OsStr::new("print"), OsStr::new(&target)])?;
    if !output.status.success() {
        return Ok(Loaded::No);
    }
    let pid = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = ")?.parse().ok());
    Ok(Loaded::Yes { pid })
}

/// `launchctl bootstrap`, retried briefly on `5: Input/output error`, which launchd also returns
/// while a just-booted-out job of the same label is still being torn down.
fn bootstrap(plist: &Path) -> Result<(), StartupError> {
    let domain = domain();
    let mut retries = BOOTSTRAP_RETRIES;
    loop {
        match launchctl("bootstrap", &[OsStr::new(&domain), plist.as_os_str()]) {
            Err(StartupError::Launchctl {
                code: Some(EIO_EXIT),
                ..
            }) if retries > 0 => {
                retries -= 1;
                thread::sleep(BOOTSTRAP_RETRY_DELAY);
            }
            other => return other,
        }
    }
}

fn bootout() -> Result<(), StartupError> {
    let target = format!("{}/{LABEL}", domain());
    launchctl("bootout", &[OsStr::new(&target)])
}

fn launchctl(action: &'static str, args: &[&OsStr]) -> Result<(), StartupError> {
    let mut full = vec![OsStr::new(action)];
    full.extend_from_slice(args);
    let output = run_launchctl(&full)?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(StartupError::Launchctl {
        action,
        status: output.status.to_string(),
        code: output.status.code(),
        detail,
    })
}

fn run_launchctl(args: &[&OsStr]) -> Result<Output, StartupError> {
    let program = launchctl_program();
    Command::new(&program)
        .args(args)
        .output()
        .map_err(|source| StartupError::Spawn { program, source })
}

/// Debug builds only: `KOSSE_TEST_LAUNCHCTL` replaces `/bin/launchctl`, so tests (which also point
/// `HOME` at a temporary directory) never load, unload or query the developer's real agents.
fn launchctl_program() -> String {
    #[cfg(debug_assertions)]
    if let Some(program) = std::env::var_os("KOSSE_TEST_LAUNCHCTL") {
        return program.to_string_lossy().into_owned();
    }
    LAUNCHCTL.to_owned()
}

/// Temp file in the same directory, then rename: launchd never reads a partial plist.
fn write_atomically(path: &Path, contents: &str) -> Result<(), StartupError> {
    let failed = |source| StartupError::Write {
        path: path.to_owned(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("/"));
    fs::create_dir_all(dir).map_err(failed)?;
    let temporary = dir.join(format!(".{LABEL}.plist.{}", std::process::id()));
    let written = fs::write(&temporary, contents).and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(failed(error));
    }
    Ok(())
}

fn in_build_directory(executable: &Path) -> bool {
    let mut components = executable.components().rev().skip(1);
    matches!(
        (components.next(), components.next()),
        (Some(profile), Some(target))
            if target.as_os_str() == "target"
                && matches!(profile.as_os_str().to_str(), Some("debug" | "release"))
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{escape, in_build_directory, render_plist};

    #[test]
    fn the_plist_escapes_the_program_path() {
        let plist = render_plist("/Users/a&b/<x>/kosse");
        assert!(plist.contains("<string>/Users/a&amp;b/&lt;x&gt;/kosse</string>"));
        assert!(plist.contains("<string>--no-open</string>"));
        assert_eq!(escape(r#"'"'"#), "&apos;&quot;&apos;");
    }

    #[test]
    fn build_directories_are_recognised() {
        assert!(in_build_directory(Path::new(
            "/src/kosse/target/release/kosse"
        )));
        assert!(in_build_directory(Path::new(
            "/src/kosse/target/debug/kosse"
        )));
        assert!(!in_build_directory(Path::new("/Users/me/.local/bin/kosse")));
        assert!(!in_build_directory(Path::new("/opt/homebrew/bin/kosse")));
    }
}
