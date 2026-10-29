//! Command-line surface. Only commands whose behavior exists are declared here.

use clap::{Parser, Subcommand};

pub const DEFAULT_PORT: u16 = 47831;

/// Local homepage for the development services already running on this machine.
///
/// Without a command, runs the menu-bar app: the tray menu plus the dashboard server, opening the
/// dashboard window. If the menu-bar app already runs on the port, brings its window forward.
#[derive(Debug, Parser)]
#[command(name = "kosse", version, about, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub app: AppArgs,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the dashboard window without a resident app: shows the menu-bar app's window when it
    /// runs on the port, or serves the dashboard until the window closes.
    Open(OpenArgs),
    /// Serve the dashboard and API in the foreground until interrupted (no tray).
    Serve(ServeArgs),
    /// Print the TCP listeners on this machine and their processes: the running instance's
    /// snapshot, or a one-shot scan when none runs.
    List(ListArgs),
    /// Project profiles: the schema, the configuration file, moved projects.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Check the platform, configuration, port, git and login startup; changes nothing. Exits 1
    /// when a check fails.
    Doctor(DoctorArgs),
    /// Start the menu-bar app at login (a per-user launch agent running this kosse with
    /// `--no-open`), stop doing so, or show whether it does. Off until enabled.
    #[command(subcommand)]
    Startup(StartupCommand),
    /// Internal: the dashboard window (Rune connected to a running instance's loopback API), started
    /// by the app and `kosse open`.
    #[command(hide = true)]
    Window(WindowArgs),
}

#[derive(Debug, clap::Args)]
pub struct AppArgs {
    /// Do not open the dashboard window at launch (for example when started at login).
    #[arg(long)]
    pub no_open: bool,

    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Debug, clap::Args)]
pub struct OpenArgs {
    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Debug, clap::Args)]
pub struct WindowArgs {
    /// The dashboard URL: `http://127.0.0.1:<port>/…` only.
    pub url: String,
    /// The window title.
    #[arg(long, default_value = crate::launcher::TITLE)]
    pub title: String,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the JSON Schema for a project profile (`kosse.json`); save it wherever your editor can
    /// reference it. Kossé never fetches `$schema`.
    Schema,
    /// Print where central profiles are saved (`$KOSSE_CONFIG_DIR` overrides the directory).
    Path,
    /// Re-attach a saved profile to a project's new directory.
    ///
    /// Central profiles are keyed by the project's canonical root path and never follow a project
    /// by name, so a moved or re-cloned project shows its old profile as "project not observed"
    /// until it is moved here explicitly. Refuses when no profile is saved for the old root or the
    /// new one already has one.
    Move {
        /// The old project root, exactly as saved (see `kosse list`).
        from: String,
        /// The project's new root directory.
        to: std::path::PathBuf,
    },
}

#[derive(Debug, clap::Args)]
pub struct DoctorArgs {
    /// Print the checks as JSON.
    #[arg(long)]
    pub json: bool,

    /// The port whose occupant to check.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}

#[derive(Debug, Subcommand)]
pub enum StartupCommand {
    /// Write `~/Library/LaunchAgents/com.davidpp.kosse.plist` for this kosse and load it (which
    /// also starts the menu-bar app now, or finds the one already running).
    Enable,
    /// Unload the agent (stopping a Kossé that launchd started) and remove its plist.
    Disable,
    /// Show whether the agent exists, which kosse it runs and whether launchd has it loaded.
    Status,
}

#[derive(Debug, clap::Args)]
pub struct ListArgs {
    /// Print the scan as JSON: the versioned `Snapshot` contract; see its `schemaVersion`.
    #[arg(long)]
    pub json: bool,

    /// The port of the running instance to ask; scans once when no Kossé answers there (or with
    /// 0).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Do not open the native dashboard window.
    #[arg(long)]
    pub no_open: bool,

    /// Development only (`bun run dev`): also answer for the Vite dev server's authority. Not
    /// present in release builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    pub dev: bool,

    #[command(flatten)]
    pub server: ServerArgs,
}

/// Where an instance listens, plus the debug-only test scopes.
#[derive(Debug, clap::Args)]
pub struct ServerArgs {
    /// Loopback port to listen on (0 picks a free port).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Tests only: probe these PIDs automatically and nothing else (`--probe-only=` probes
    /// nothing), so a test never probes the machine's real services. Not present in release
    /// builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true, value_name = "PID,…", value_parser = parse_pids)]
    pub probe_only: Option<PidList>,

    /// Tests only: stop/force may signal only these processes (matched by PID and the start time
    /// read at startup) and their direct children (`--act-only=` none), so a test can never
    /// signal the machine's real services. Not present in release builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true, value_name = "PID,…", value_parser = parse_action_pids)]
    pub act_only: Option<PidList>,
}

/// A comma-separated PID list; empty is allowed.
#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
pub struct PidList(pub Vec<u32>);

#[cfg(debug_assertions)]
fn parse_pids(value: &str) -> Result<PidList, String> {
    value
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            part.trim()
                .parse()
                .map_err(|_| format!("{part:?} is not a PID"))
        })
        .collect::<Result<_, _>>()
        .map(PidList)
}

/// Like [`parse_pids`], refusing PID 0 and 1.
#[cfg(debug_assertions)]
fn parse_action_pids(value: &str) -> Result<PidList, String> {
    let pids = parse_pids(value)?;
    match pids.0.iter().find(|&&pid| pid <= 1) {
        Some(pid) => Err(format!("PID {pid} can never be signalled")),
        None => Ok(pids),
    }
}
