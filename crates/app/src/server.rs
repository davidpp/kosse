//! HTTP surface of a running instance.
//!
//! Routes: the API (see [`api`]); any other `/api` path is 404; everything else is the embedded
//! web bundle (see [`assets`]). Every route, the bundle included, passes [`guard::enforce`].
//! [`bind`] claims the loopback endpoint and refuses ports another process already serves; [`run`]
//! serves until SIGINT/SIGTERM, ends open event streams, then drains for a bounded time.

use std::{
    future::IntoFuture,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    middleware,
    routing::{any, get, post},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
};

use crate::{
    api::{self, AppState},
    assets,
    customize::{self, Customize},
    guard::{self, MAX_BODY_BYTES},
};

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

pub fn router(state: AppState, customize: Customize) -> Router {
    let policy = std::sync::Arc::clone(&state.policy);
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/session", get(api::session))
        .route("/api/snapshot", get(api::snapshot))
        .route("/api/events", get(api::events))
        .route("/api/refresh", post(api::refresh))
        .route("/api/probe", post(api::probe))
        .route("/api/stop", post(api::stop))
        .route("/api/force", post(api::force))
        .route("/api/profile", post(customize::save).with_state(customize))
        .route("/api", any(StatusCode::NOT_FOUND))
        .route("/api/{*rest}", any(StatusCode::NOT_FOUND))
        .fallback_service(get(assets::serve))
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(policy, guard::enforce))
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

/// Serves until SIGINT/SIGTERM, then sets `stopping` (ending event streams) and gives open
/// connections [`SHUTDOWN_GRACE`] to finish.
pub async fn run(
    listener: TcpListener,
    app: Router,
    stopping: watch::Sender<bool>,
) -> Result<(), ServeError> {
    let (stop_requested, stop_received) = oneshot::channel();
    let serving = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            stopping.send_replace(true);
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
            report!(
                "kosse: dropping connections still open after {}s",
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

#[cfg(test)]
mod tests {
    //! The HTTP surface over a real socket, with scan outcomes scripted where the OS cannot
    //! produce them on demand (an enumeration failure after a success).

    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpStream},
        sync::Arc,
        time::Duration,
    };

    use kosse_core::model::{Observation, Snapshot};
    use tokio::sync::watch;

    use super::*;
    use crate::{
        actions::{self, Actions},
        guard::{ActionToken, Policy},
        prober::{Probes, Scope},
        scheduler::{MakeScan, Scheduler},
    };

    fn observation() -> Observation {
        Observation {
            scanned_at_ms: kosse_core::scan::unix_millis(std::time::SystemTime::now()),
            scan_duration_ms: 0.1,
            services: Vec::new(),
            projects: Vec::new(),
            warnings: Vec::new(),
            profiles: Vec::new(),
        }
    }

    /// Succeeds once, then fails every time.
    fn succeed_then_fail() -> MakeScan {
        Box::new(|| {
            let mut scans = 0;
            Box::new(move || {
                scans += 1;
                if scans == 1 {
                    Ok(observation())
                } else {
                    Err("enumeration failed".to_owned())
                }
            })
        })
    }

    fn request(addr: SocketAddr, head: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(addr).expect("connect");
        write!(
            stream,
            "{head}\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .expect("write");
        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("read");
        let status = raw
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status");
        let body = raw.split_once("\r\n\r\n").map(|(_, body)| body.to_owned());
        (status, body.unwrap_or_default())
    }

    #[test]
    fn a_failing_scanner_serves_the_last_good_observation_marked_stale() {
        let scheduler =
            Scheduler::start_with(Duration::from_secs(3600), succeed_then_fail()).expect("start");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let token = ActionToken::generate().expect("token");
        let policy = Arc::new(Policy::new(addr.port(), false, token));
        let token = policy.token().to_owned();
        let (stopping, stopping_receiver) = watch::channel(false);
        let probes = Arc::new(Probes::new(Scope::Only(std::collections::HashSet::new())));
        let actions = Arc::new(Actions::new(actions::Scope::Only(
            std::collections::HashMap::new(),
        )));
        let app = router(
            AppState::new(scheduler.handle(), policy, probes, actions, stopping_receiver),
            Customize::new(
                scheduler.handle(),
                kosse_core::profiles::CentralStore::new("/var/empty/kosse-tests".into()),
            ),
        );
        runtime.spawn(async move { axum::serve(listener, app).await });

        let (status, body) = request(addr, "GET /api/snapshot HTTP/1.1", "");
        assert_eq!(status, 200, "{body}");
        let good: Snapshot = serde_json::from_str(&body).expect("snapshot");
        assert_eq!(good.scanner.failure, None);

        std::thread::sleep(Duration::from_millis(20));
        let (status, body) = request(
            addr,
            &format!(
                "POST /api/refresh HTTP/1.1\r\nOrigin: http://{addr}\r\nContent-Type: application/json\r\nX-Kosse-Token: {token}"
            ),
            "{}",
        );
        assert_eq!(status, 200, "{body}");
        let failed: Snapshot = serde_json::from_str(&body).expect("snapshot");
        assert_eq!(
            failed.scanner.failure.as_deref(),
            Some("enumeration failed")
        );
        assert_eq!(failed.scanner.consecutive_failures, 1);
        assert_eq!(
            failed.observation, good.observation,
            "last good kept, not emptied"
        );
        let observed_at = failed
            .observation
            .as_ref()
            .expect("observation")
            .scanned_at_ms;
        assert!(
            observed_at < failed.scanner.last_attempt_at_ms,
            "older than the attempt"
        );
        assert!(failed.revision > good.revision);

        // Every later reader sees the same stale state until a scan succeeds.
        let (_, body) = request(addr, "GET /api/snapshot HTTP/1.1", "");
        let latest: Snapshot = serde_json::from_str(&body).expect("snapshot");
        assert_eq!(latest, failed);

        stopping.send_replace(true);
        assert!(scheduler.shutdown(Duration::from_secs(1)));
    }
}
