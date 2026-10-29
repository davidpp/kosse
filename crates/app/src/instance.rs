//! Finding the Kossé instance already serving a port, so a second launch uses it instead of
//! starting a second scanner, and asking a resident one to show its window.
//!
//! An instance is trusted only when both hold:
//! - the socket listening on `127.0.0.1:<port>` belongs to a process of this user (checked
//!   through the same socket enumeration and process metadata as every scan, before anything is
//!   sent to it), so another user's Kossé, or its action token, is never used;
//! - it answers the API as Kossé does: `GET /api/health` names `kosse` (and its mode), and
//!   `GET /api/snapshot` returns a snapshot with an instance ID.
//!
//! There is no PID or lock file to go stale; anything else on the port is refused. This defends
//! against mistakes and other users, not against a process of the same user that imitates the API
//! on purpose (see the guard's threat model).

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    Method, Request, StatusCode,
    body::Bytes,
    header::{ACCEPT, CONNECTION, CONTENT_TYPE, HOST, ORIGIN, USER_AGENT},
};
use hyper_util::rt::TokioIo;
use kosse_core::{
    contract::{Health, InstanceMode, Session, WindowRequest},
    inspect::{self, Presence},
    model::{SNAPSHOT_SCHEMA_VERSION, Snapshot},
    sockets,
};
use tokio::net::TcpStream;

use crate::{guard::TOKEN_HEADER, route::Route};

/// Connect, request and response together, per request. A local instance answers in
/// milliseconds; a busy one's first snapshot can take a scan.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// Snapshots of thousands of listeners stay well below this.
const BODY_LIMIT: usize = 32 * 1024 * 1024;

/// What answers on `127.0.0.1:<port>`.
#[derive(Debug)]
pub enum Found {
    /// Nothing listens on either loopback address.
    Nothing,
    /// A verified Kossé instance of this user.
    Kosse(Box<Instance>),
    /// Something that is never trusted or reused.
    Refused(InstanceError),
}

#[derive(Debug)]
pub struct Instance {
    pub url: String,
    pub addr: SocketAddr,
    pub version: String,
    pub mode: InstanceMode,
    /// Its current snapshot: the instance ID, and `kosse list`'s data when compatible.
    pub snapshot: Option<Snapshot>,
}

impl Instance {
    /// Takes the snapshot, if this build can read it as its own (same schema version).
    pub fn compatible_snapshot(&mut self) -> Option<Snapshot> {
        self.snapshot
            .take()
            .filter(|snapshot| snapshot.schema_version == SNAPSHOT_SCHEMA_VERSION)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error(
        "127.0.0.1:{0} is in use by a process that is not Kossé; stop it or choose another port with --port"
    )]
    NotKosse(u16),
    #[error(
        "127.0.0.1:{0} is held by another user's process (or one whose owner cannot be read); Kossé never uses another user's instance, so choose another port with --port"
    )]
    OtherUser(u16),
    #[error(
        "the Kossé at {0} is a `kosse open` session, which stops when its window closes; close that window, then start kosse again"
    )]
    Ephemeral(String),
    #[error("could not ask the Kossé at {url} to show its window: {reason}")]
    Window { url: String, reason: String },
}

pub async fn find(port: u16) -> Found {
    let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
    let (v4_open, v6_open) = tokio::join!(accepts(v4), accepts(v6));
    if !v4_open {
        // Kossé binds 127.0.0.1 only; a listener on [::1] alone is something else.
        return if v6_open {
            Found::Refused(InstanceError::NotKosse(port))
        } else {
            Found::Nothing
        };
    }
    let ours = tokio::task::spawn_blocking(move || listener_is_ours(port))
        .await
        .unwrap_or(false);
    if !ours {
        return Found::Refused(InstanceError::OtherUser(port));
    }
    match verify(v4).await {
        Some(instance) => Found::Kosse(Box::new(instance)),
        None => Found::Refused(InstanceError::NotKosse(port)),
    }
}

/// Asks a resident instance to show its window at `route` (a same-origin `POST /api/window`, as
/// the dashboard would send it).
pub async fn show_window(instance: &Instance, route: Route) -> Result<(), InstanceError> {
    let failed = |reason: String| InstanceError::Window {
        url: instance.url.clone(),
        reason,
    };
    let body = serde_json::to_vec(&WindowRequest {
        route: route.into(),
    })
    .map_err(|error| failed(error.to_string()))?;
    match post(instance, "/api/window", body, REQUEST_TIMEOUT).await {
        Ok((StatusCode::NO_CONTENT, _)) => Ok(()),
        Ok((status, body)) => Err(failed(format!(
            "{status} {}",
            String::from_utf8_lossy(&body)
        ))),
        Err(error) => Err(failed(error.to_string())),
    }
}

/// Why [`post`] has no answer.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    /// The session token could not be read, so nothing was posted.
    #[error("no session token: {0}")]
    Session(String),
    /// The request may have reached the instance, but no answer came back within the timeout.
    #[error("no answer")]
    NoAnswer,
}

/// A same-origin JSON `POST` of `body` to `path`, as the dashboard sends it: the session token
/// from `GET /api/session` first, then the request with the instance's exact `Host` and
/// `Origin`, `Content-Type: application/json` and the token. `timeout` bounds the `POST`.
pub async fn post(
    instance: &Instance,
    path: &str,
    body: Vec<u8>,
    timeout: Duration,
) -> Result<(StatusCode, Bytes), PostError> {
    let (status, session) = request(
        instance.addr,
        Method::GET,
        "/api/session",
        None,
        REQUEST_TIMEOUT,
    )
    .await
    .ok_or_else(|| PostError::Session("no answer".to_owned()))?;
    if status != StatusCode::OK {
        return Err(PostError::Session(format!("answered {status}")));
    }
    let session: Session =
        serde_json::from_slice(&session).map_err(|error| PostError::Session(error.to_string()))?;
    request(
        instance.addr,
        Method::POST,
        path,
        Some((&session.token, body)),
        timeout,
    )
    .await
    .ok_or(PostError::NoAnswer)
}

async fn accepts(addr: SocketAddr) -> bool {
    matches!(
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

/// Whether every process this user can see listening on `127.0.0.1:<port>` (or the wildcard,
/// which also answers there) runs as this user. Another user's sockets are not visible without
/// administrator rights, so seeing none means "not ours".
fn listener_is_ours(port: u16) -> bool {
    let Ok(sockets) = sockets::tcp_listen_sockets() else {
        return false;
    };
    let owners: Vec<Option<u32>> = sockets
        .iter()
        .filter(|socket| {
            socket.port == port
                && (socket.address == IpAddr::V4(Ipv4Addr::LOCALHOST)
                    || socket.address.is_unspecified())
        })
        .map(|socket| effective_user(socket.pid))
        .collect();
    owned_by(&owners, effective_user(std::process::id()))
}

fn effective_user(pid: u32) -> Option<u32> {
    match inspect::vitals(pid) {
        Presence::Running(vitals) => vitals.effective_user_id,
        Presence::Exited | Presence::Unknown => None,
    }
}

/// At least one owner, and every one known and equal to ours.
fn owned_by(owners: &[Option<u32>], own: Option<u32>) -> bool {
    own.is_some() && !owners.is_empty() && owners.iter().all(|owner| *owner == own)
}

async fn verify(addr: SocketAddr) -> Option<Instance> {
    let (status, body) = request(addr, Method::GET, "/api/health", None, REQUEST_TIMEOUT).await?;
    let health: Health = (status == StatusCode::OK)
        .then(|| serde_json::from_slice(&body).ok())
        .flatten()?;
    if health.name != env!("CARGO_PKG_NAME") {
        return None;
    }
    // Any Kossé names its instance; a newer schema still identifies one, so this only requires
    // the field, and `compatible_snapshot` decides whether `list` can use the rest.
    let (status, body) = request(addr, Method::GET, "/api/snapshot", None, REQUEST_TIMEOUT).await?;
    if status != StatusCode::OK {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let instance = value.get("instance")?.as_str()?;
    if instance.is_empty() {
        return None;
    }
    Some(Instance {
        url: format!("http://{addr}/"),
        addr,
        version: health.version,
        mode: health.mode,
        snapshot: serde_json::from_value(value).ok(),
    })
}

/// One request bounded by `timeout`, with the instance's own `Host`; a `POST` carries the token
/// and the same-origin `Origin` the guard requires. The status and a body within [`BODY_LIMIT`].
async fn request(
    addr: SocketAddr,
    method: Method,
    path: &str,
    post: Option<(&str, Vec<u8>)>,
    timeout: Duration,
) -> Option<(StatusCode, Bytes)> {
    tokio::time::timeout(timeout, async {
        let stream = TcpStream::connect(addr).await.ok()?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .ok()?;
        let driver = tokio::spawn(connection);
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, addr.to_string())
            .header(USER_AGENT, concat!("kosse/", env!("CARGO_PKG_VERSION")))
            .header(ACCEPT, "application/json")
            .header(CONNECTION, "close");
        let body = match post {
            Some((token, body)) => {
                builder = builder
                    .header(ORIGIN, format!("http://{addr}"))
                    .header(CONTENT_TYPE, "application/json")
                    .header(TOKEN_HEADER, token);
                body
            }
            None => Vec::new(),
        };
        let request = builder.body(Full::new(Bytes::from(body))).ok()?;
        let response = sender.send_request(request).await.ok()?;
        let status = response.status();
        let body = Limited::new(response.into_body(), BODY_LIMIT)
            .collect()
            .await
            .ok()
            .map(http_body_util::Collected::to_bytes);
        driver.abort();
        Some((status, body?))
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::owned_by;

    #[test]
    fn only_listeners_all_owned_by_this_user_are_ours() {
        assert!(owned_by(&[Some(501)], Some(501)));
        assert!(owned_by(&[Some(501), Some(501)], Some(501)));
        // Another user's socket (or one invisible to us), an unreadable owner, or not knowing
        // who we are: never ours.
        assert!(!owned_by(&[], Some(501)));
        assert!(!owned_by(&[Some(502)], Some(501)));
        assert!(!owned_by(&[Some(501), Some(0)], Some(501)));
        assert!(!owned_by(&[None], Some(501)));
        assert!(!owned_by(&[Some(501)], None));
    }
}
