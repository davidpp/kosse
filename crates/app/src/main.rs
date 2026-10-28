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

mod api;
mod assets;
mod cli;
mod guard;
mod list;
mod prober;
mod scheduler;
mod server;

use std::{io, process::ExitCode, sync::Arc};

use clap::Parser;

use crate::{
    api::AppState,
    cli::{Cli, Command, ServeArgs},
    guard::{ActionToken, Policy},
    list::ListError,
    prober::{Probes, Scope},
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
    #[error("could not create the action token: {0}")]
    Token(#[source] io::Error),
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
    let token = ActionToken::generate().map_err(AppError::Token)?;
    let policy = Arc::new(Policy::new(addr.port(), dev_mode(&args), token));
    // The only scanner of this instance; every route reads its snapshots.
    let probes = Arc::new(Probes::new(probe_scope(&args)));
    let scheduler =
        Scheduler::start(SCAN_INTERVAL, Arc::clone(&probes)).map_err(AppError::Scanner)?;
    let (stopping, stopping_receiver) = tokio::sync::watch::channel(false);
    let probing = tokio::spawn(prober::run(
        Arc::clone(&probes),
        scheduler.handle().subscribe(),
        stopping_receiver.clone(),
    ));
    let app = server::router(AppState::new(
        scheduler.handle(),
        policy,
        probes,
        stopping_receiver,
    ));
    report!("stackscan {} listening on {url}", env!("CARGO_PKG_VERSION"));
    if !args.no_open {
        open_in_browser(&url);
    }
    let served = server::run(listener, app, stopping).await;
    probing.abort();
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

/// Development mode (admitting the Vite dev server's authority) exists only in debug builds.
fn dev_mode(args: &ServeArgs) -> bool {
    #[cfg(debug_assertions)]
    {
        if args.dev {
            report!(
                "stackscan: development mode: also answering for {}",
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

/// Automatic probing covers every candidate, except in debug builds given `--probe-only`.
fn probe_scope(args: &ServeArgs) -> Scope {
    #[cfg(debug_assertions)]
    if let Some(pids) = &args.probe_only {
        return Scope::Only(pids.0.iter().copied().collect());
    }
    let _ = args;
    Scope::All
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
