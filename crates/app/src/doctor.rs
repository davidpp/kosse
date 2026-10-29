//! `kosse doctor`: what Kossé needs and what it would find, without changing anything.
//!
//! Every check only reads: the platform, this executable and the one `PATH` resolves, the central
//! configuration (parsed, never rewritten), who answers on the port (the same verification as a
//! second launch: socket owner first, then `GET` requests to a verified Kossé only), `git` from the
//! developer directory, the login startup agent and the GUI session. Exit status 1 when a check
//! fails (Kossé cannot work as installed), 0 otherwise, warnings included.

use std::{
    fmt::Write as _,
    fs,
    io::{self, Write as _},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

use kosse_core::{
    git::Git,
    profiles::{CentralStore, central::CONFIG_VERSION},
};
use serde_json::json;

use crate::{
    cli::DoctorArgs,
    instance::{self, Found},
    startup,
};

const SW_VERS: &str = "/usr/bin/sw_vers";
const CONSOLE: &str = "/dev/console";
const LSOF: &str = "/usr/sbin/lsof";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Info,
    Warn,
    Fail,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

struct Check {
    name: &'static str,
    level: Level,
    detail: String,
}

fn check(name: &'static str, level: Level, detail: impl Into<String>) -> Check {
    Check {
        name,
        level,
        detail: detail.into(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DoctorError {
    #[error("{0} failed")]
    Failed(String),
    #[error("could not write the output: {0}")]
    Output(#[source] io::Error),
}

pub fn run(args: &DoctorArgs, runtime: &tokio::runtime::Runtime) -> Result<(), DoctorError> {
    let executable = std::env::current_exe().ok();
    let checks = [
        platform(),
        binary(executable.as_deref()),
        config(),
        runtime.block_on(port(args.port)),
        git(),
        lsof(),
        login_startup(executable.as_deref()),
        session(),
    ];
    let output = if args.json {
        render_json(&checks)
    } else {
        render_text(&checks)
    };
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            return Err(DoctorError::Output(error));
        }
        _ => {}
    }
    let failed: Vec<&str> = checks
        .iter()
        .filter(|check| check.level == Level::Fail)
        .map(|check| check.name)
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(DoctorError::Failed(format!("check {}", failed.join(", "))))
    }
}

fn render_text(checks: &[Check]) -> String {
    let mut text = format!("Kossé {} doctor\n", env!("CARGO_PKG_VERSION"));
    for check in checks {
        let _ = writeln!(
            text,
            "{:<5} {:<10} {}",
            check.level.label(),
            check.name,
            check.detail
        );
    }
    text
}

fn render_json(checks: &[Check]) -> String {
    let checks: Vec<_> = checks
        .iter()
        .map(|check| json!({"name": check.name, "level": check.level.label(), "detail": check.detail}))
        .collect();
    let ok = !checks.iter().any(|check| check["level"] == "fail");
    format!(
        "{}\n",
        json!({"version": env!("CARGO_PKG_VERSION"), "ok": ok, "checks": checks})
    )
}

fn platform() -> Check {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    if (os, arch) != ("macos", "aarch64") {
        return check(
            "platform",
            Level::Fail,
            format!("{os} {arch}: Kossé v1 supports only macOS on Apple silicon"),
        );
    }
    let version = command_line(SW_VERS, &["-productVersion"]).unwrap_or_else(|| "?".to_owned());
    check(
        "platform",
        Level::Ok,
        format!("macOS {version} on Apple silicon"),
    )
}

fn binary(executable: Option<&Path>) -> Check {
    let version = env!("CARGO_PKG_VERSION");
    let Some(executable) = executable else {
        return check(
            "binary",
            Level::Warn,
            format!("kosse {version}; could not locate this executable"),
        );
    };
    let here = format!("kosse {version} at {}", executable.display());
    match on_path() {
        None => check("binary", Level::Info, format!("{here}; no kosse on PATH")),
        Some(found) if same_file(&found, executable) => check("binary", Level::Ok, here),
        Some(found) => check(
            "binary",
            Level::Warn,
            format!("{here}; but `kosse` on PATH runs {}", found.display()),
        ),
    }
}

fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("kosse"))
        .find(|candidate| {
            fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn config() -> Check {
    let store = match CentralStore::from_env() {
        Ok(store) => store,
        Err(error) => return check("config", Level::Fail, error.to_string()),
    };
    let file = store.file();
    let dir = file.parent().unwrap_or(Path::new("/"));
    let dir_mode = match fs::metadata(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return check(
                "config",
                Level::Ok,
                format!(
                    "{}: none yet (created when a profile is first saved)",
                    file.display()
                ),
            );
        }
        Err(error) => {
            return check("config", Level::Fail, format!("{}: {error}", dir.display()));
        }
        Ok(meta) => meta.permissions().mode(),
    };
    let file_mode = fs::metadata(&file)
        .ok()
        .map(|meta| meta.permissions().mode());
    let loaded = match store.load() {
        Ok(None) => "no profiles saved yet".to_owned(),
        Ok(Some((config, _))) => format!(
            "{} saved profile(s), format version {} (this build writes {CONFIG_VERSION})",
            config.projects.len(),
            config.version
        ),
        Err(problem) => {
            return check(
                "config",
                Level::Fail,
                format!(
                    "{}: {problem}; Kossé will not overwrite it, so saving profiles is unavailable until it is fixed or removed",
                    file.display()
                ),
            );
        }
    };
    let open = |mode: u32| mode & 0o077 != 0;
    if open(dir_mode) || file_mode.is_some_and(open) {
        return check(
            "config",
            Level::Warn,
            format!(
                "{}: {loaded}; readable by other users (Kossé writes the directory 0700 and the file 0600)",
                file.display()
            ),
        );
    }
    check("config", Level::Ok, format!("{}: {loaded}", file.display()))
}

async fn port(port: u16) -> Check {
    if port == 0 {
        return check(
            "port",
            Level::Info,
            "--port 0: a free port is picked at start",
        );
    }
    match instance::find(port).await {
        Found::Nothing => check("port", Level::Ok, format!("127.0.0.1:{port} is free")),
        Found::Kosse(found) => {
            let mode = serde_json::to_value(found.mode)
                .ok()
                .and_then(|mode| mode.as_str().map(str::to_owned))
                .unwrap_or_default();
            check(
                "port",
                Level::Ok,
                format!(
                    "Kossé {} ({mode}) is running at {}",
                    found.version, found.url
                ),
            )
        }
        Found::Refused(error) => check("port", Level::Warn, error.to_string()),
    }
}

fn git() -> Check {
    match Git::locate() {
        Some(git) => check(
            "git",
            Level::Ok,
            format!("{} (developer directory)", git.executable().display()),
        ),
        None => check(
            "git",
            Level::Warn,
            "no git in the active developer directory (`xcode-select --install`): services inside Git worktrees are reported without a project (attributionUnavailable)",
        ),
    }
}

fn lsof() -> Check {
    let present = Path::new(LSOF).is_file();
    check(
        "lsof",
        Level::Info,
        if present {
            format!("{LSOF} present (optional: Kossé does not use it; its tests do)")
        } else {
            format!("{LSOF} missing (optional: Kossé does not use it; its tests do)")
        },
    )
}

fn login_startup(executable: Option<&Path>) -> Check {
    let status = match startup::status() {
        Ok(status) => status,
        Err(error) => return check("startup", Level::Warn, error.to_string()),
    };
    let detail = status.describe();
    let level = match &status.program {
        None if status.loaded == startup::Loaded::No => Level::Info,
        None | Some(Err(_)) => Level::Warn,
        Some(Ok(program)) if !program.is_file() => Level::Warn,
        Some(Ok(program)) if executable.is_some_and(|here| !same_file(program, here)) => {
            return check(
                "startup",
                Level::Info,
                format!("{detail}; another kosse than this one"),
            );
        }
        Some(Ok(_)) => Level::Ok,
    };
    check("startup", level, detail)
}

/// The tray and the window need this user's GUI login: the console belongs to the user logged in
/// at the screen (`root` at the login window), whichever session this command runs from (SSH,
/// a terminal, launchd).
fn session() -> Check {
    let console = fs::metadata(CONSOLE).map(|meta| meta.uid());
    match console {
        Ok(uid) if uid == rustix::process::getuid().as_raw() => check(
            "gui",
            Level::Ok,
            "this user is logged in at the screen: the menu-bar app and the dashboard window can start",
        ),
        _ => check(
            "gui",
            Level::Warn,
            "this user is not logged in at the screen: the menu-bar app and the window need a GUI login; `kosse serve` and `kosse list` work without",
        ),
    }
}

/// The first line a short read-only command prints, if it succeeded.
fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()
    })
}
