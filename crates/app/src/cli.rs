//! Command-line surface. Only commands whose behavior exists are declared here.

use clap::{Parser, Subcommand};

pub const DEFAULT_PORT: u16 = 47831;

/// Local homepage for the development services already running on this machine.
#[derive(Debug, Parser)]
#[command(name = "kosse", version, about, arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve the dashboard and API in the foreground until interrupted.
    Serve(ServeArgs),
    /// Scan once and print the TCP listeners on this machine and their processes.
    List(ListArgs),
    /// Project profiles: the schema, the configuration file, moved projects.
    #[command(subcommand)]
    Config(ConfigCommand),
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
pub struct ListArgs {
    /// Print the scan as JSON: the versioned `Snapshot` contract; see its `schemaVersion`.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Loopback port to listen on (0 picks a free port).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Do not open the dashboard in the default browser.
    #[arg(long)]
    pub no_open: bool,

    /// Development only (`bun run dev`): also answer for the Vite dev server's authority. Not
    /// present in release builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    pub dev: bool,

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
