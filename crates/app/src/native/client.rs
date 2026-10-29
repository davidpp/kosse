//! All native mutations cross the same Host/Origin/token boundary as other clients.
use super::State;
use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request, body::Bytes, header::HOST};
use hyper_util::rt::TokioIo;
use kosse_core::{
    action::{ActionRequest, ActionResult},
    contract::{ApiError, Preferences, ProbeRequest, SaveProfileRequest, Session},
    model::Snapshot,
};
use rune::AppHandle;
use serde::{Serialize, de::DeserializeOwned};
use std::{net::SocketAddr, time::Duration};
use tokio::sync::mpsc;

pub enum Command {
    Save(SaveProfileRequest),
    Refresh,
    Preferences(bool),
    Probe(ProbeRequest),
    Action(ActionRequest, bool),
}

async fn get<T: DeserializeOwned>(addr: SocketAddr, path: &str) -> Result<T, String> {
    let (status, body) = crate::instance::request(addr, Method::GET, path, None)
        .await
        .ok_or("Kossé is disconnected. Waiting to reconnect…")?;
    if !status.is_success() {
        return Err(format!("Kossé returned {status}."));
    }
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

async fn post<T: Serialize>(addr: SocketAddr, path: &str, value: &T) -> Result<Vec<u8>, String> {
    if !matches!(
        crate::instance::find(addr.port()).await,
        crate::instance::Found::Kosse(_)
    ) {
        return Err("Kossé owner could not be verified. Nothing was sent.".into());
    }
    let session: Session = get(addr, "/api/session").await?;
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let (status, body) = crate::instance::request(
        addr,
        Method::POST,
        path,
        Some((&session.token, body)),
    )
    .await
    .ok_or("Connection interrupted. The action outcome is unknown; refresh before trying again.")?;
    if !status.is_success() {
        return Err(serde_json::from_slice::<ApiError>(&body)
            .map_or_else(|_| format!("Request refused: {status}"), |e| e.message));
    }
    Ok(body.to_vec())
}

pub async fn run(
    addr: SocketAddr,
    mut commands: mpsc::UnboundedReceiver<Command>,
    ui: AppHandle<State>,
) {
    if !matches!(
        crate::instance::find(addr.port()).await,
        crate::instance::Found::Kosse(_)
    ) {
        ui.update(|s, _| s.error = Some("The window could not verify its Kossé owner.".into()));
        return;
    }
    if let Ok(preferences) = get::<Preferences>(addr, "/api/preferences").await {
        ui.update(move |s, _| s.all = preferences.all_listeners);
    }
    let feed = tokio::spawn(feed(addr, ui.clone()));
    while let Some(command) = commands.recv().await {
        let result = match command {
            Command::Save(request) => match post(addr, "/api/profile", &request).await {
                Ok(body) => {
                    ui.update(|s, _| s.editor = None);
                    Ok(body)
                }
                Err(error) => {
                    ui.update(move |s, _| {
                        if let Some(editor) = &mut s.editor {
                            editor.busy = false;
                            editor.error = Some(error);
                        }
                    });
                    Ok(Vec::new())
                }
            },
            Command::Refresh => post(addr, "/api/refresh", &serde_json::json!({})).await,
            Command::Preferences(all_listeners) => {
                post(addr, "/api/preferences", &Preferences { all_listeners }).await
            }
            Command::Probe(request) => post(addr, "/api/probe", &request).await,
            Command::Action(request, force) => {
                let response = post(
                    addr,
                    if force { "/api/force" } else { "/api/stop" },
                    &request,
                )
                .await;
                match response {
                    Ok(body) => match serde_json::from_slice::<ActionResult>(&body) {
                        Ok(result) => {
                            ui.update(move |s, _| {
                                if let Some(dialog) = &mut s.confirmation {
                                    dialog.busy = false;
                                    dialog.result = Some(result);
                                }
                            });
                            Ok(body)
                        }
                        Err(error) => Err(error.to_string()),
                    },
                    Err(error) => Err(error),
                }
            }
        };
        if let Err(error) = result {
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

async fn feed(addr: SocketAddr, ui: AppHandle<State>) {
    loop {
        let result = events(addr, &ui).await;
        if let Err(error) = result {
            ui.update(move |s, _| {
                s.connected = false;
                s.connection_error = Some(error);
            });
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn publish(ui: &AppHandle<State>, snapshot: Snapshot) {
    ui.update_if(move |s, _| {
        let reconnected = !s.connected;
        s.connected = true;
        s.connection_error = None;
        if s.snapshot
            .as_ref()
            .is_none_or(|old| old.instance != snapshot.instance || old.revision < snapshot.revision)
        {
            s.accept(snapshot);
            return true;
        }
        reconnected
    });
}

async fn events(addr: SocketAddr, ui: &AppHandle<State>) -> Result<(), String> {
    if !matches!(
        crate::instance::find(addr.port()).await,
        crate::instance::Found::Kosse(_)
    ) {
        return Err("Connection lost. Waiting for a verified Kossé instance…".into());
    }
    publish(ui, get::<Snapshot>(addr, "/api/snapshot").await?);
    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| e.to_string())?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| e.to_string())?;
    let driver = tokio::spawn(connection);
    let result = async {
        let request = Request::builder()
            .uri("/api/events")
            .header(HOST, addr.to_string())
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
            let frame = tokio::time::timeout(Duration::from_secs(25), body.frame())
                .await
                .map_err(|_| "Live updates timed out.")?
                .ok_or("Live connection closed.")?
                .map_err(|e| e.to_string())?;
            if let Ok(data) = frame.into_data() {
                buffer.extend_from_slice(&data);
                if buffer.len() > 32 * 1024 * 1024 {
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
                        publish(ui, snapshot);
                    }
                }
            }
        }
    }
    .await;
    driver.abort();
    result
}
