//! `stackscan` entry point: parses the command line, owns the async runtime, maps failures to
//! exit codes (0 success, 1 runtime failure, 2 usage error via clap).

mod assets;
mod cli;
mod server;

use std::{io, process::ExitCode};

use clap::Parser;

use crate::{
    cli::{Cli, Command, ServeArgs},
    server::ServeError,
};

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("could not start the async runtime: {0}")]
    Runtime(#[source] io::Error),
    #[error(transparent)]
    Serve(#[from] ServeError),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stackscan: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    // Built explicitly rather than via #[tokio::main]: the macOS tray must later own the main thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(AppError::Runtime)?;

    match cli.command {
        Command::Serve(args) => runtime.block_on(serve(args)),
    }
}

async fn serve(args: ServeArgs) -> Result<(), AppError> {
    let listener = server::bind(args.port).await?;
    let addr = listener.local_addr().map_err(|source| ServeError::Bind {
        addr: (std::net::Ipv4Addr::LOCALHOST, args.port).into(),
        source,
    })?;
    let url = format!("http://{addr}/");
    eprintln!("stackscan {} listening on {url}", env!("CARGO_PKG_VERSION"));
    if !args.no_open {
        open_in_browser(&url);
    }
    server::run(listener).await?;
    eprintln!("stackscan stopped");
    Ok(())
}

/// Best effort: the server is already useful without a browser window, so failures only warn.
fn open_in_browser(url: &str) {
    let opened = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status();
    match opened {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("stackscan: could not open {url} in a browser ({status})"),
        Err(error) => eprintln!("stackscan: could not open {url} in a browser: {error}"),
    }
}
