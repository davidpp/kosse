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
}

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Loopback port to listen on (0 picks a free port).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Do not open the dashboard in the default browser.
    #[arg(long)]
    pub no_open: bool,
}
