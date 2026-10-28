//! `kosse open`: the dashboard window without a resident app.
//!
//! - The menu-bar app runs on the port: it is asked to show its own window (`POST /api/window`)
//!   and this exits; there is never a second window or scanner.
//! - Another Kossé runs there (`serve`, or another `open`): it has no window to show, so the
//!   default browser opens its dashboard, with the reason.
//! - Nothing runs there: this serves the dashboard until its window closes, then exits, so
//!   nothing stays running.

use std::time::Duration;

use kosse_core::contract::InstanceMode;
use tokio::sync::oneshot;

use crate::{
    AppError,
    cli::OpenArgs,
    instance::{self, Instance},
    launcher::{self, Window},
    route::Route,
    running::{self, Claim},
};

/// How often `open` checks whether the window has closed.
const WINDOW_POLL: Duration = Duration::from_millis(100);

pub fn run(args: &OpenArgs) -> Result<(), AppError> {
    let runtime = crate::runtime()?;
    let running = match running::claim(&runtime, &args.server, InstanceMode::Ephemeral, None)? {
        Claim::Existing(existing) => return show_existing(&runtime, &existing),
        Claim::Started(running) => running,
    };
    let url = running.url().to_owned();
    report!("kosse {} listening on {url}", env!("CARGO_PKG_VERSION"));
    let (quit, quit_requested) = oneshot::channel::<()>();
    let result = runtime.block_on(async move {
        let mut server = tokio::spawn(running.run(async move {
            let _ = quit_requested.await;
        }));
        let window = match Window::spawn(&url, launcher::TITLE, None) {
            Ok(window) => window,
            Err(error) => {
                launcher::fallback(
                    &url,
                    &format!("could not start the dashboard window ({error})"),
                );
                report!("kosse: serving {url} until interrupted");
                return server.await;
            }
        };
        loop {
            tokio::select! {
                // SIGINT/SIGTERM stopped the server: close the window too.
                result = &mut server => {
                    window.close();
                    return result;
                }
                () = tokio::time::sleep(WINDOW_POLL) => if window.exited() { break },
            }
        }
        let started = window.started();
        // Waits for the browser fallback, if the window never started.
        window.close();
        if started {
            let _ = quit.send(());
        } else {
            // The browser fallback has opened the dashboard; it needs the server.
            report!("kosse: serving {url} until interrupted");
        }
        server.await
    });
    result.map_err(|_| AppError::Stopped)??;
    report!("kosse stopped");
    Ok(())
}

/// The menu-bar app shows its own window; any other instance gets a browser tab.
fn show_existing(runtime: &tokio::runtime::Runtime, existing: &Instance) -> Result<(), AppError> {
    let url = &existing.url;
    match existing.mode {
        InstanceMode::Resident => {
            runtime.block_on(instance::show_window(existing, Route::Dashboard))?;
            report!("kosse: showing the window of the Kossé menu-bar app at {url}");
        }
        InstanceMode::Ephemeral | InstanceMode::Headless => {
            let kind = if existing.mode == InstanceMode::Headless {
                "`kosse serve`"
            } else {
                "another `kosse open`"
            };
            launcher::fallback(
                url,
                &format!("the Kossé at {url} is {kind}, which has no window to show"),
            );
        }
    }
    Ok(())
}
