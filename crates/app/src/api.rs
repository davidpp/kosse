//! API handlers. They read the scheduler's shared snapshot and never scan; the writes ask the
//! scheduler for a scan or the prober for one probe, and are rate-limited.
//!
//! - `GET /api/health`: name and version.
//! - `GET /api/session`: the action token for unsafe requests ([`Session`]).
//! - `GET /api/snapshot`: the latest [`Snapshot`].
//! - `GET /api/events`: Server-Sent Events, one `snapshot` event per publication, starting with
//!   the current one. A slow client skips to the newest snapshot rather than queueing old ones;
//!   a reconnecting client gets the current snapshot first and orders by `instance` + `revision`.
//! - `POST /api/refresh` (`{}`): scans now and returns the resulting snapshot.
//! - `POST /api/probe` ([`ProbeRequest`]): probes one endpoint now ("try HTTP") and returns its
//!   `HttpProbe`, marked `requested`.
//!
//! Each snapshot is serialized once per revision and shared by every reader and event stream.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::Bytes,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures_util::stream::{self, Stream};
use kosse_core::{
    contract::{ApiErrorCode, Health, ProbeRequest, Session},
    model::Snapshot,
    probe,
};
use tokio::sync::watch;

use crate::{
    guard::{Policy, Refusal},
    prober::Probes,
    scheduler::{RefreshError, SchedulerHandle},
};

/// How long `GET /api/snapshot` waits for the first scan of a fresh instance.
const FIRST_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
/// Minimum time between scans triggered through `POST /api/refresh`, and between requested
/// probes.
pub const WRITE_MIN_INTERVAL: Duration = Duration::from_secs(1);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct AppState {
    scheduler: SchedulerHandle,
    pub policy: Arc<Policy>,
    probes: Arc<Probes>,
    refresh_limit: Arc<RateLimit>,
    probe_limit: Arc<RateLimit>,
    json: Arc<SnapshotJson>,
    /// Turns true when the server starts shutting down; open event streams then end.
    stopping: watch::Receiver<bool>,
}

impl AppState {
    pub fn new(
        scheduler: SchedulerHandle,
        policy: Arc<Policy>,
        probes: Arc<Probes>,
        stopping: watch::Receiver<bool>,
    ) -> Self {
        Self {
            scheduler,
            policy,
            probes,
            refresh_limit: Arc::new(RateLimit::default()),
            probe_limit: Arc::new(RateLimit::default()),
            json: Arc::new(SnapshotJson::default()),
            stopping,
        }
    }
}

/// At most one accepted call per [`WRITE_MIN_INTERVAL`].
#[derive(Default)]
struct RateLimit {
    last: Mutex<Option<Instant>>,
}

impl RateLimit {
    fn try_acquire(&self) -> bool {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < WRITE_MIN_INTERVAL) {
            return false;
        }
        *last = Some(now);
        true
    }
}

/// The JSON text of the latest snapshot served, keyed by revision (one scheduler per state).
/// `Bytes` clones share one buffer.
#[derive(Default)]
struct SnapshotJson {
    latest: Mutex<Option<(u64, Bytes)>>,
}

impl SnapshotJson {
    fn of(&self, snapshot: &Snapshot) -> Result<Bytes, serde_json::Error> {
        let mut latest = self.latest.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((revision, json)) = latest.as_ref()
            && *revision == snapshot.revision
        {
            return Ok(json.clone());
        }
        let json = Bytes::from(serde_json::to_vec(snapshot)?);
        *latest = Some((snapshot.revision, json.clone()));
        Ok(json)
    }
}

pub async fn health() -> Json<Health> {
    Json(Health {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

pub async fn session(State(state): State<AppState>) -> Json<Session> {
    Json(Session {
        token: state.policy.token().to_owned(),
    })
}

pub async fn snapshot(State(state): State<AppState>) -> Response {
    if let Some(snapshot) = state.scheduler.latest() {
        return snapshot_response(&state, &snapshot);
    }
    // A fresh instance: its first scan is normally milliseconds away.
    let mut changes = state.scheduler.subscribe();
    let first = tokio::time::timeout(FIRST_SNAPSHOT_TIMEOUT, changes.wait_for(Option::is_some));
    let latest = match first.await {
        Ok(Ok(latest)) => latest.clone(),
        Ok(Err(_)) => return scanner_stopped(),
        Err(_) => return scanner_busy(),
    };
    match latest {
        Some(snapshot) => snapshot_response(&state, &snapshot),
        None => scanner_busy(),
    }
}

fn snapshot_response(state: &AppState, snapshot: &Snapshot) -> Response {
    match state.json.of(snapshot) {
        Ok(json) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            json,
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, serde_json::Error>>> {
    let stream = stream::unfold(
        (
            state.scheduler.subscribe(),
            state.stopping,
            state.json,
            true,
        ),
        |(mut changes, mut stopping, json, first)| async move {
            if first {
                let current = changes.borrow_and_update().clone();
                if let Some(snapshot) = current {
                    let event = event(&json, &snapshot);
                    return Some((event, (changes, stopping, json, false)));
                }
            }
            loop {
                let published = tokio::select! {
                    changed = changes.changed() => changed.is_ok(),
                    stopped = stopping.wait_for(|stopping| *stopping) => {
                        drop(stopped);
                        false
                    }
                };
                if !published {
                    return None;
                }
                let latest = changes.borrow_and_update().clone();
                if let Some(snapshot) = latest {
                    let event = event(&json, &snapshot);
                    return Some((event, (changes, stopping, json, false)));
                }
            }
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

fn event(json: &SnapshotJson, snapshot: &Snapshot) -> Result<Event, serde_json::Error> {
    let json = json.of(snapshot)?;
    // serde_json writes UTF-8.
    Ok(Event::default()
        .event("snapshot")
        .data(String::from_utf8_lossy(&json)))
}

/// The refresh request body must be an empty JSON object (reserved for options).
pub async fn refresh(
    State(state): State<AppState>,
    body: Result<Json<serde_json::Map<String, serde_json::Value>>, JsonRejection>,
) -> Response {
    match body {
        Ok(Json(options)) if options.is_empty() => {}
        Ok(_) => return invalid_payload(),
        Err(rejection) => return body_refusal(&rejection),
    }
    if !state.refresh_limit.try_acquire() {
        return rate_limited(
            "a refresh ran less than a second ago; the snapshot stream has its result",
        );
    }
    match state.scheduler.refresh().await {
        Ok(snapshot) => snapshot_response(&state, &snapshot),
        Err(RefreshError::TimedOut) => scanner_busy(),
        Err(RefreshError::Stopped) => scanner_stopped(),
    }
}

pub async fn probe(
    State(state): State<AppState>,
    body: Result<Json<ProbeRequest>, JsonRejection>,
) -> Response {
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) => return body_refusal(&rejection),
    };
    let Some(snapshot) = state.scheduler.latest() else {
        return scanner_busy();
    };
    let endpoint = snapshot.observation.as_ref().and_then(|observation| {
        observation
            .services
            .iter()
            .find(|service| service.process.identity == request.identity)
            .and_then(|service| {
                service
                    .endpoints
                    .iter()
                    .find(|endpoint| endpoint.port == request.port)
            })
    });
    let Some(endpoint) = endpoint else {
        return Refusal::new(
            StatusCode::CONFLICT,
            ApiErrorCode::StaleIdentity,
            "that process and port are not in the current snapshot; refresh and retry",
        )
        .into_response();
    };
    let Some(target) = probe::requested_target(endpoint) else {
        return Refusal::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiErrorCode::NotProbeable,
            "Kossé never probes another user's process or a non-loopback endpoint",
        )
        .into_response();
    };
    if !state.probe_limit.try_acquire() {
        return rate_limited("a probe was requested less than a second ago");
    }
    let key = (request.identity, request.port);
    Json(state.probes.request(key, target).await).into_response()
}

/// A body the JSON extractor refused: too large (declared or while reading a chunked body), or
/// not the expected JSON.
fn body_refusal(rejection: &JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return Refusal::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ApiErrorCode::PayloadTooLarge,
            "request body too large",
        )
        .into_response();
    }
    invalid_payload()
}

fn invalid_payload() -> Response {
    Refusal::new(
        StatusCode::BAD_REQUEST,
        ApiErrorCode::InvalidPayload,
        "the body does not match the request type",
    )
    .into_response()
}

fn rate_limited(message: &'static str) -> Response {
    let mut response = Refusal::new(
        StatusCode::TOO_MANY_REQUESTS,
        ApiErrorCode::RateLimited,
        message,
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    response
}

fn scanner_busy() -> Response {
    Refusal::new(
        StatusCode::SERVICE_UNAVAILABLE,
        ApiErrorCode::ScannerBusy,
        "no scan finished in time; the last snapshot (if any) still stands",
    )
    .into_response()
}

fn scanner_stopped() -> Response {
    Refusal::new(
        StatusCode::SERVICE_UNAVAILABLE,
        ApiErrorCode::ScannerStopped,
        "the scanner has stopped; restart Kossé",
    )
    .into_response()
}
