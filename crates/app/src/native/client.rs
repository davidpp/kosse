//! All native mutations cross the same Host/Origin/token boundary as other clients.
//!
//! The window is its owner's child (over stdin, and it exits when that pipe closes), so the owner
//! is verified once, at startup, through [`crate::instance::find`]: socket ownership and the API.
//! Afterwards the window only checks that the snapshots it reads still name that instance; it
//! never enumerates sockets again. The action token is fetched once and refetched only when the
//! guard refuses it.
use super::State;
use crate::{api::KEEP_ALIVE, instance::BODY_LIMIT};
use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request, body::Bytes, header::HOST};
use hyper_util::rt::TokioIo;
use kosse_core::{
    action::{ActionRequest, ActionResult},
    contract::{ApiError, ApiErrorCode, Preferences, ProbeRequest, SaveProfileRequest, Session},
    model::Snapshot,
};
use rune::AppHandle;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, mpsc};

/// A stream silent for longer than this missed its keep-alives: reconnect.
const LIVE_SILENCE: Duration = KEEP_ALIVE.saturating_add(Duration::from_secs(10));
/// Pause between reconnection attempts.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub enum Command {
    Save(SaveProfileRequest),
    Refresh,
    Preferences(bool),
    Probe(ProbeRequest),
    Action(ActionRequest, bool),
}

/// The verified owner: its address, instance ID and cached action token.
struct Owner {
    addr: SocketAddr,
    instance: String,
    /// Set when a snapshot names another instance: nothing is sent from then on.
    replaced: AtomicBool,
    token: Mutex<Option<String>>,
}

impl Owner {
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let (status, body) = crate::instance::request(self.addr, Method::GET, path, None)
            .await
            .ok_or("Kossé is disconnected. Waiting to reconnect…")?;
        if !status.is_success() {
            return Err(format!("Kossé returned {status}."));
        }
        serde_json::from_slice(&body).map_err(|e| e.to_string())
    }

    /// A same-origin `POST` with the cached token. A refused token is refetched and the request
    /// sent once more: the guard refused it before any handler ran, so nothing happened.
    async fn post<T: Serialize>(&self, path: &str, value: &T) -> Result<Vec<u8>, String> {
        if self.replaced.load(Ordering::Acquire) {
            return Err("Another Kossé now answers on this port. Nothing was sent.".into());
        }
        let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        let mut cached = self.token.lock().await;
        for attempt in 0..2 {
            let token = if let Some(token) = cached.as_ref() {
                token.clone()
            } else {
                let session: Session = self.get("/api/session").await?;
                cached.insert(session.token).clone()
            };
            let (status, response) = crate::instance::request(
                self.addr,
                Method::POST,
                path,
                Some((&token, body.clone())),
            )
            .await
            .ok_or(
                "Connection interrupted. The action outcome is unknown; refresh before trying again.",
            )?;
            if status.is_success() {
                return Ok(response.to_vec());
            }
            let refusal = serde_json::from_slice::<ApiError>(&response).ok();
            if attempt == 0
                && refusal
                    .as_ref()
                    .is_some_and(|e| e.error == ApiErrorCode::TokenRequired)
            {
                *cached = None;
                continue;
            }
            return Err(refusal.map_or_else(|| format!("Request refused: {status}"), |e| e.message));
        }
        Err("The action token was refused twice. Nothing was sent.".into())
    }
}

pub async fn run(
    addr: SocketAddr,
    mut commands: mpsc::UnboundedReceiver<Command>,
    ui: AppHandle<State>,
) {
    let crate::instance::Found::Kosse(verified) = crate::instance::find(addr.port()).await else {
        ui.update(|s, _| s.error = Some("The window could not verify its Kossé owner.".into()));
        return;
    };
    let Some(instance) = verified.snapshot.map(|snapshot| snapshot.instance) else {
        ui.update(|s, _| s.error = Some("The Kossé owner sent an unreadable snapshot.".into()));
        return;
    };
    let owner = Arc::new(Owner {
        addr,
        instance,
        replaced: AtomicBool::new(false),
        token: Mutex::new(None),
    });
    if let Ok(preferences) = owner.get::<Preferences>("/api/preferences").await {
        ui.update(move |s, _| s.all = preferences.all_listeners);
    }
    let feed = tokio::spawn(feed(Arc::clone(&owner), ui.clone()));
    while let Some(command) = commands.recv().await {
        if let Err(error) = execute(&owner, &ui, command).await {
            ui.update(move |s, _| {
                if let Some(dialog) = &mut s.confirmation {
                    dialog.busy = false;
                    dialog.error = Some(error);
                } else {
                    s.error = Some(error);
                }
            });
        }
    }
    feed.abort();
}

/// Sends one command. A failed save is reported in the editor; other failures are returned.
async fn execute(owner: &Owner, ui: &AppHandle<State>, command: Command) -> Result<(), String> {
    match command {
        Command::Save(request) => {
            let saved = owner.post("/api/profile", &request).await;
            ui.update(move |s, _| match saved {
                Ok(_) => s.editor = None,
                Err(error) => {
                    if let Some(editor) = &mut s.editor {
                        editor.busy = false;
                        editor.error = Some(error);
                    }
                }
            });
        }
        Command::Refresh => {
            owner.post("/api/refresh", &serde_json::json!({})).await?;
        }
        Command::Preferences(all_listeners) => {
            owner
                .post("/api/preferences", &Preferences { all_listeners })
                .await?;
        }
        Command::Probe(request) => {
            owner.post("/api/probe", &request).await?;
        }
        Command::Action(request, force) => {
            let path = if force { "/api/force" } else { "/api/stop" };
            let body = owner.post(path, &request).await?;
            let result =
                serde_json::from_slice::<ActionResult>(&body).map_err(|e| e.to_string())?;
            ui.update(move |s, _| {
                if let Some(dialog) = &mut s.confirmation {
                    dialog.busy = false;
                    dialog.result = Some(result);
                }
            });
        }
    }
    Ok(())
}

async fn feed(owner: Arc<Owner>, ui: AppHandle<State>) {
    loop {
        if let Err(error) = events(&owner, &ui).await {
            ui.update(move |s, _| {
                s.connected = false;
                s.connection_error = Some(error);
            });
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

/// Publishes a snapshot of the verified instance; one naming another instance stops all sends.
fn publish(owner: &Owner, ui: &AppHandle<State>, snapshot: Snapshot) -> Result<(), String> {
    if snapshot.instance != owner.instance {
        owner.replaced.store(true, Ordering::Release);
        return Err("Another Kossé now answers on this port. Reopen Kossé.".into());
    }
    ui.update_if(move |s, _| {
        let reconnected = !s.connected;
        s.connected = true;
        s.connection_error = None;
        if s.snapshot
            .as_ref()
            .is_none_or(|old| old.revision < snapshot.revision)
        {
            s.accept(snapshot);
            return true;
        }
        reconnected
    });
    Ok(())
}

async fn events(owner: &Owner, ui: &AppHandle<State>) -> Result<(), String> {
    publish(owner, ui, owner.get::<Snapshot>("/api/snapshot").await?)?;
    let stream = tokio::net::TcpStream::connect(owner.addr)
        .await
        .map_err(|e| e.to_string())?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| e.to_string())?;
    let driver = tokio::spawn(connection);
    let result = async {
        let request = Request::builder()
            .uri("/api/events")
            .header(HOST, owner.addr.to_string())
            .body(Empty::<Bytes>::new())
            .map_err(|e| e.to_string())?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("Live updates refused: {}", response.status()));
        }
        let mut body = response.into_body();
        let mut buffer = Vec::new();
        loop {
            let frame = tokio::time::timeout(LIVE_SILENCE, body.frame())
                .await
                .map_err(|_| "Live updates timed out.")?
                .ok_or("Live connection closed.")?
                .map_err(|e| e.to_string())?;
            let Ok(data) = frame.into_data() else {
                continue;
            };
            buffer.extend_from_slice(&data);
            if buffer.len() > BODY_LIMIT {
                return Err("Live update exceeded the size limit.".into());
            }
            while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<_> = buffer.drain(..=end).collect();
                if let Some(data) = line
                    .strip_prefix(b"data: ")
                    .or_else(|| line.strip_prefix(b"data:"))
                {
                    let snapshot = serde_json::from_slice(data)
                        .map_err(|e| format!("Invalid live update: {e}"))?;
                    publish(owner, ui, snapshot)?;
                }
            }
        }
    }
    .await;
    driver.abort();
    result
}
