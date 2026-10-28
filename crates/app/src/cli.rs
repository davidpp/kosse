//! Command-line surface. Only commands whose behavior exists are declared here.

use clap::{Parser, Subcommand};

pub const DEFAULT_PORT: u16 = 47831;

/// Local homepage for the development services already running on this machine.
#[derive(Debug, Parser)]
#[command(name = "stackscan", version, about, arg_required_else_help = true)]
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
