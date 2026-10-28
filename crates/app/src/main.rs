//! `kosse` entry point: parses the command line, picks the mode (menu-bar app, `open`, `serve`,
//! `list`, `config`, the internal `window`), maps failures to exit codes (0 success, 1 runtime
//! failure, 2 usage error via clap).

/// `eprintln!` without its panic: diagnostics to a closed stderr (`kosse … 2>&-`, or a
/// pipe whose reader exited) are dropped instead of aborting the process.
macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod actions;
mod activation;
mod api;
mod assets;
mod cli;
mod config;
mod customize;
mod guard;
mod instance;
mod launcher;
mod list;
mod menu;
mod open;
mod opener;
mod prober;
mod route;
mod running;
mod scheduler;
mod server;
mod text;
mod tray;
mod window;

use std::{io, process::ExitCode};

use clap::Parser;
use kosse_core::{contract::InstanceMode, profiles::StoreError};

use crate::{
    cli::{Cli, Command, ServeArgs},
    list::ListError,
    running::Running,
    server::ServeError,
    tray::TrayError,
};

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("could not start the async runtime: {0}")]
    Runtime(#[source] io::Error),
    #[error(transparent)]
    Serve(#[from] ServeError),
    #[error(transparent)]
    List(#[from] ListError),
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not start the scanner: {0}")]
    Scanner(#[source] io::Error),
    #[error("could not create the action token: {0}")]
    Token(#[source] io::Error),
    #[error(transparent)]
    Tray(#[from] TrayError),
    #[error(transparent)]
    Instance(#[from] instance::InstanceError),
    #[error("the server task stopped unexpectedly")]
    Stopped,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // The window reports a failure to its owner, which says why; it writes no stderr line itself.
    if let Some(Command::Window(args)) = &cli.command {
        return window::run(args);
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report!("kosse: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        None => tray::run(&cli.app),
        Some(Command::Open(args)) => open::run(&args),
        Some(Command::Serve(args)) => runtime()?.block_on(serve(args)),
        Some(Command::List(args)) => Ok(list::run(&args)?),
        Some(Command::Config(command)) => Ok(config::run(&command)?),
        Some(Command::Window(_)) => Ok(()),
    }
}

/// Built explicitly rather than via `#[tokio::main]`: the tray and the window own the main thread.
fn runtime() -> Result<tokio::runtime::Runtime, AppError> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)
}

async fn serve(args: ServeArgs) -> Result<(), AppError> {
    let running =
        Running::start(&args.server, dev_mode(&args), InstanceMode::Headless, None).await?;
    let url = running.url().to_owned();
    report!("kosse {} listening on {url}", env!("CARGO_PKG_VERSION"));
    if !args.no_open {
        opener::open_or_report(&url);
    }
    running.run(std::future::pending()).await?;
    report!("kosse stopped");
    Ok(())
}

/// Development mode (admitting the Vite dev server's authority) exists only in debug builds.
fn dev_mode(args: &ServeArgs) -> bool {
    #[cfg(debug_assertions)]
    {
        if args.dev {
            report!(
                "kosse: development mode: also answering for {}",
                guard::DEV_AUTHORITY
            );
        }
        args.dev
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = args;
        false
    }
}
