//! `stackscan` entry point: parses the command line, owns the async runtime, maps failures to
//! exit codes (0 success, 1 runtime failure, 2 usage error via clap).

/// `eprintln!` without its panic: diagnostics to a closed stderr (`stackscan … 2>&-`, or a
/// pipe whose reader exited) are dropped instead of aborting the process.
macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod assets;
mod cli;
mod list;
mod scheduler;
mod server;

use std::{io, process::ExitCode};

use clap::Parser;

use crate::{
    cli::{Cli, Command, ServeArgs},
    list::ListError,
    scheduler::{SCAN_INTERVAL, Scheduler},
    server::ServeError,
};

/// How long shutdown waits for a scan in progress (normally milliseconds).
const SCANNER_STOP_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("could not start the async runtime: {0}")]
    Runtime(#[source] io::Error),
    #[error(transparent)]
    Serve(#[from] ServeError),
    #[error(transparent)]
    List(#[from] ListError),
    #[error("could not start the scanner: {0}")]
    Scanner(#[source] io::Error),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report!("stackscan: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Command::Serve(args) => {
            // Built explicitly rather than via #[tokio::main]: the macOS tray must later own the
            // main thread.
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(AppError::Runtime)?;
            runtime.block_on(serve(args))
        }
        Command::List(args) => Ok(list::run(&args)?),
    }
}

async fn serve(args: ServeArgs) -> Result<(), AppError> {
    let listener = server::bind(args.port).await?;
    let addr = listener.local_addr().map_err(|source| ServeError::Bind {
        addr: (std::net::Ipv4Addr::LOCALHOST, args.port).into(),
        source,
    })?;
    let url = format!("http://{addr}/");
    report!("stackscan {} listening on {url}", env!("CARGO_PKG_VERSION"));
    // The only scanner of this instance; S04's API reads its snapshots.
    let scheduler = Scheduler::start(SCAN_INTERVAL).map_err(AppError::Scanner)?;
    if !args.no_open {
        open_in_browser(&url);
    }
    let served = server::run(listener).await;
    // Waiting for a scan in progress blocks, so keep it off the async workers.
    let stopped = tokio::task::spawn_blocking(move || scheduler.shutdown(SCANNER_STOP_DEADLINE))
        .await
        .unwrap_or(false);
    if !stopped {
        report!("stackscan: the scanner is still busy; exiting without waiting for it");
    }
    served?;
    report!("stackscan stopped");
    Ok(())
}

/// Best effort: the server is already useful without a browser window, so failures only warn.
fn open_in_browser(url: &str) {
    let opened = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status();
    match opened {
        Ok(status) if status.success() => {}
        Ok(status) => report!("stackscan: could not open {url} in a browser ({status})"),
        Err(error) => report!("stackscan: could not open {url} in a browser: {error}"),
    }
}
