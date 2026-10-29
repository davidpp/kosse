//! A temporary scanner owner and its Rune window; existing owners refocus their one window.
use std::sync::Arc;

use kosse_core::contract::InstanceMode;
use tokio::sync::{mpsc, oneshot};

use crate::{
    AppError,
    cli::OpenArgs,
    instance::{self, Instance},
    launcher::{self, Notice, Window},
    route::Route,
    running::{self, Claim},
};

pub fn run(args: &OpenArgs) -> Result<(), AppError> {
    let runtime = crate::runtime()?;
    let (routes, mut incoming) = mpsc::unbounded_channel();
    let sink = Arc::new(move |route| {
        let _ = routes.send(route);
    });
    let running = match running::claim(&runtime, &args.server, InstanceMode::Ephemeral, Some(sink))?
    {
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
        // The window's output reader says when it ended; nothing polls for it.
        let (exits, mut exited) = mpsc::unbounded_channel();
        let notify: launcher::Notify = Arc::new(move |notice| {
            if matches!(notice, Notice::Exited) {
                let _ = exits.send(());
            }
        });
        let mut window = match Window::spawn(&url, Route::Dashboard, launcher::TITLE, Some(notify))
        {
            Ok(window) => window,
            Err(error) => {
                launcher::fallback(
                    &url,
                    &format!("could not start the dashboard window ({error})"),
                );
                let _ = quit.send(());
                let _ = server.await;
                return Err(AppError::Window(error.to_string()));
            }
        };
        loop {
            tokio::select! {
                // SIGINT/SIGTERM stopped the server: close the window too.
                result = &mut server => {
                    window.close();
                    return result.map_err(|_| AppError::Stopped)?;
                }
                Some(route) = incoming.recv() => {
                    window.show(Route::from(route), launcher::TITLE);
                }
                _ = exited.recv() => break,
            }
        }
        let started = window.started();
        window.close();
        let _ = quit.send(());
        server.await.map_err(|_| AppError::Stopped)??;
        if !started {
            return Err(AppError::Window("the native window did not start".into()));
        }
        Ok(())
    });
    result?;
    report!("kosse stopped");
    Ok(())
}

/// The owner on the port shows its own one window (`POST /api/window`).
fn show_existing(runtime: &tokio::runtime::Runtime, existing: &Instance) -> Result<(), AppError> {
    runtime.block_on(instance::show_window(existing, Route::Dashboard))?;
    report!("kosse: showing the native window at {}", existing.url);
    Ok(())
}
