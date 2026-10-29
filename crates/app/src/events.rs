//! A client of a running instance's `GET /api/events` (Server-Sent Events): one `snapshot`
//! event per publication, the current one first. Snapshots arrive in `instance` + `revision`
//! order; an older or repeated one is skipped.

use std::{net::SocketAddr, time::Duration};

use http_body_util::{BodyExt, Empty};
use hyper::{
    Method, Request, StatusCode,
    body::{Bytes, Incoming},
    header::{ACCEPT, HOST, USER_AGENT},
};
use hyper_util::rt::TokioIo;
use kosse_core::model::Snapshot;
use tokio::{net::TcpStream, task::JoinHandle};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// One event holds one snapshot; `instance.rs` reads snapshots up to the same size.
const EVENT_LIMIT: usize = 32 * 1024 * 1024;

pub struct Events {
    body: Incoming,
    buffer: Vec<u8>,
    /// How far `buffer` has been searched for an event's end, so each byte is searched once.
    searched: usize,
    /// The newest `(instance, revision)` returned; older ones are skipped.
    last: Option<(String, u64)>,
    driver: JoinHandle<Result<(), hyper::Error>>,
}

impl Drop for Events {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

impl Events {
    pub async fn open(addr: SocketAddr) -> Result<Self, String> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| "connection timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| error.to_string())?;
        let driver = tokio::spawn(connection);
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/events")
            .header(HOST, addr.to_string())
            .header(USER_AGENT, concat!("kosse/", env!("CARGO_PKG_VERSION")))
            .header(ACCEPT, "text/event-stream")
            .body(Empty::<Bytes>::new())
            .map_err(|error| error.to_string())?;
        let response = tokio::time::timeout(CONNECT_TIMEOUT, sender.send_request(request))
            .await
            .map_err(|_| "no response".to_owned())?
            .map_err(|error| error.to_string())?;
        if response.status() != StatusCode::OK {
            driver.abort();
            return Err(format!("the event stream answered {}", response.status()));
        }
        Ok(Self {
            body: response.into_body(),
            buffer: Vec::new(),
            searched: 0,
            last: None,
            driver,
        })
    }

    /// The next newer snapshot; `None` when the stream ended.
    pub async fn next(&mut self) -> Result<Option<Snapshot>, String> {
        loop {
            // Back one byte: the previous chunk may have ended on the first `\n`.
            while let Some(found) = self.buffer[self.searched.saturating_sub(1)..]
                .windows(2)
                .position(|pair| pair == b"\n\n")
            {
                let end = self.searched.saturating_sub(1) + found;
                let event: Vec<u8> = self.buffer.drain(..end + 2).collect();
                self.searched = 0;
                if let Some(snapshot) = self.snapshot_of(&event)? {
                    return Ok(Some(snapshot));
                }
            }
            match self.body.frame().await {
                None => return Ok(None),
                Some(Err(error)) => return Err(error.to_string()),
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.searched = self.buffer.len();
                        self.buffer.extend_from_slice(&data);
                        if self.buffer.len() > EVENT_LIMIT {
                            return Err("an event exceeds 32 MiB".to_owned());
                        }
                    }
                }
            }
        }
    }

    /// The snapshot of one event, unless it is a keep-alive or not newer than the last one.
    fn snapshot_of(&mut self, event: &[u8]) -> Result<Option<Snapshot>, String> {
        let text = String::from_utf8_lossy(event);
        if !text.lines().any(|line| line == "event: snapshot") {
            return Ok(None);
        }
        let data: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| data.strip_prefix(' ').unwrap_or(data))
            .collect();
        let snapshot: Snapshot =
            serde_json::from_str(&data.join("\n")).map_err(|error| error.to_string())?;
        if let Some((instance, revision)) = &self.last
            && *instance == snapshot.instance
            && snapshot.revision <= *revision
        {
            return Ok(None);
        }
        self.last = Some((snapshot.instance.clone(), snapshot.revision));
        Ok(Some(snapshot))
    }
}
