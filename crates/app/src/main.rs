//! `kosse` entry point: parses the command line, picks the mode (menu-bar app, `open`, `serve`,
//! `list`, `config`, `doctor`, `startup`, the internal `window`), maps failures to exit codes (0
//! success, 1 runtime failure, 2 usage error via clap).

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
mod doctor;
mod guard;
mod instance;
mod launcher;
mod list;
mod menu;
mod native;
mod open;
mod opener;
mod preferences;
mod present;
mod prober;
mod route;
mod running;
mod scheduler;
mod server;
mod startup;
mod text;
mod tray;
#[cfg(debug_assertions)]
mod window_control;

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
    #[error("doctor: {0}")]
    Doctor(#[from] doctor::DoctorError),
    #[error("startup: {0}")]
    Startup(#[from] startup::StartupError),
    #[error("native window unavailable: {0}")]
    Window(String),
    #[error("the server task stopped unexpectedly")]
    Stopped,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // The window reports a failure to its owner, which says why; it writes no stderr line itself.
    if let Some(Command::Window(args)) = &cli.command {
        return native::run(args);
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
        Some(Command::Doctor(args)) => Ok(doctor::run(&args, &runtime()?)?),
        Some(Command::Startup(command)) => Ok(startup::run(&command)?),
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
    let (routes, mut incoming) = tokio::sync::mpsc::unbounded_channel();
    let sink = std::sync::Arc::new(move |route| {
        let _ = routes.send(route);
    });
    let running = Running::start(&args.server, InstanceMode::Headless, Some(sink)).await?;
    let url = running.url().to_owned();
    report!("kosse {} listening on {url}", env!("CARGO_PKG_VERSION"));
    let mut windows = launcher::Windows::new(&url, std::sync::Arc::new(|_| {}));
    if !args.no_open {
        windows.show(route::Route::Dashboard, launcher::TITLE);
    }
    let server = running.run(std::future::pending());
    tokio::pin!(server);
    let result = loop {
        tokio::select! {
            result = &mut server => break result,
            Some(route) = incoming.recv() => {
                windows.show(route::Route::from(route), launcher::TITLE);
            }
        }
    };
    windows.close();
    result?;
    report!("kosse stopped");
    Ok(())
}
