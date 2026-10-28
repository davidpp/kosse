//! HTTP surface of a running instance.
//!
//! Routes: `GET /api/health`; any other `/api` path is 404; everything else is the embedded web
//! bundle (see [`assets`]). [`bind`] claims the loopback endpoint and refuses ports another
//! process already serves; [`run`] serves until SIGINT/SIGTERM, then drains for a bounded time.

use std::{
    future::IntoFuture,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{any, get},
};
use stackscan_core::contract::Health;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

use crate::assets;

const OCCUPANCY_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
/// How long in-flight connections may keep a stopping server alive. Idle keep-alive or
/// half-sent requests would otherwise hold graceful shutdown open indefinitely.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error(
        "{addr} is already in use by another process; stop it or choose another port with --port"
    )]
    PortInUse { addr: SocketAddr },
    #[error("could not listen on {addr}: {source}")]
    Bind { addr: SocketAddr, source: io::Error },
    #[error("server stopped unexpectedly: {0}")]
    Serve(#[source] io::Error),
}

pub fn router() -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api", any(StatusCode::NOT_FOUND))
        .route("/api/{*rest}", any(StatusCode::NOT_FOUND))
        .fallback_service(get(assets::serve))
}

async fn health() -> Json<Health> {
    Json(Health {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// Binds `127.0.0.1:port`, refusing a port another process already serves on loopback.
///
/// tokio sets `SO_REUSEADDR` (so a restart is not blocked by `TIME_WAIT`), and with it macOS lets a
/// 127.0.0.1 bind succeed next to a listener on the wildcard address. A service on `[::1]` alone
/// does not conflict at all, yet owns `localhost:port` for most clients. Connect probes on both
/// loopback addresses catch these cases instead of silently shadowing the other service.
pub async fn bind(port: u16) -> Result<TcpListener, ServeError> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if port != 0 {
        let ipv6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
        let (v4_taken, v6_taken) = tokio::join!(is_accepting(addr), is_accepting(ipv6));
        if v4_taken {
            return Err(ServeError::PortInUse { addr });
        }
        if v6_taken {
            return Err(ServeError::PortInUse { addr: ipv6 });
        }
    }
    TcpListener::bind(addr).await.map_err(|source| {
        if source.kind() == io::ErrorKind::AddrInUse {
            ServeError::PortInUse { addr }
        } else {
            ServeError::Bind { addr, source }
        }
    })
}

async fn is_accepting(addr: SocketAddr) -> bool {
    matches!(
        tokio::time::timeout(OCCUPANCY_PROBE_TIMEOUT, TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

/// Serves until SIGINT/SIGTERM, then gives open connections [`SHUTDOWN_GRACE`] to finish.
pub async fn run(listener: TcpListener) -> Result<(), ServeError> {
    let (stop_requested, stop_received) = oneshot::channel();
    let serving = axum::serve(listener, router())
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = stop_requested.send(());
        })
        .into_future();
    tokio::pin!(serving);

    tokio::select! {
        result = &mut serving => return result.map_err(ServeError::Serve),
        _ = stop_received => {}
    }
    match tokio::time::timeout(SHUTDOWN_GRACE, serving).await {
        Ok(result) => result.map_err(ServeError::Serve),
        Err(_elapsed) => {
            eprintln!(
                "stackscan: dropping connections still open after {}s",
                SHUTDOWN_GRACE.as_secs()
            );
            Ok(())
        }
    }
}

/// Resolves on Ctrl-C or SIGTERM. If a handler cannot be installed that signal is simply not
/// awaited; the other one, and process termination, still stop the server.
async fn shutdown_signal() {
    let interrupt = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}
