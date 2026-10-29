//! One service row. Wide windows use one line on a shared column grid (captions above the first
//! group), so names, endpoints, metrics and actions line up across rows and groups; compact
//! layouts stack identity and actions over endpoints and labelled metrics.
use super::{
    State, display_name, observation,
    theme::{QUIET, SECONDARY, age, button, caption, mono},
};
use crate::{native::client::Command, present, text::printable};
use kosse_core::{
    contract::ProbeRequest,
    model::{CpuUsage, Endpoint, HttpProbe, ProbeResult, Service},
};
use rune::{Align, El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};

/// Column widths in points; the endpoint column takes the rest.
const IDENTITY_W: f32 = 240.0;
const CPU_W: f32 = 80.0;
const MEMORY_W: f32 = 72.0;
const AGE_W: f32 = 64.0;
const PORT_W: f32 = 52.0;
const STATUS_W: f32 = 100.0;
/// Endpoint action slot (Open or Try HTTP), reserved even when empty.
const ENDPOINT_ACTION_W: f32 = 76.0;
/// Row action slots (Inspect, Stop…), reserved even when empty so columns stay put.
const INSPECT_W: f32 = 72.0;
const STOP_W: f32 = 64.0;
const ACTION_GAP: f32 = 6.0;
const ACTIONS_W: f32 = INSPECT_W + ACTION_GAP + STOP_W;
const COLUMN_GAP: f32 = 12.0;
const ROW_PX: f32 = 14.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One line per row on the shared grid.
    Columns,
    /// Two lines: narrow windows, or beside the inspector.
    Stacked,
}

fn cell(value: impl Into<String>, width: f32, k: &Kit) -> El {
    mono(value, k).w(width).align(Align::End).truncate()
}

/// The captions over the wide grid, aligned with [`view`]'s columns.
pub fn column_captions(k: &Kit) -> El {
    row()
        .key("columns")
        .px(ROW_PX)
        .gap(COLUMN_GAP)
        .child(caption("Service", k).w(IDENTITY_W))
        .child(caption("Endpoints", k).grow(1.0).min_w(0.0))
        .child(caption("CPU", k).w(CPU_W).align(Align::End))
        .child(caption("Memory", k).w(MEMORY_W).align(Align::End))
        .child(caption("Uptime", k).w(AGE_W).align(Align::End))
        .child(el().w(ACTIONS_W))
}

pub fn view(service: &Service, s: &State, k: &Kit, layout: Layout) -> El {
    let identity = service.process.identity;
    let name = text(display_name(service, s))
        .truncate()
        .font(k.ty.ui_medium)
        .color(k.c.text_base)
        .shrink();
    let mut who = row().gap(8.0).center_y().min_w(0.0).child(name);
    if let Some(package) = &service.package {
        who = who.child(mono(printable(&package.path), k).truncate().shrink());
    }
    let metrics = Metrics::of(service, s);
    let content = match layout {
        Layout::Columns => row()
            .gap(COLUMN_GAP)
            .center_y()
            .child(who.w(IDENTITY_W))
            .child(endpoints(service, k).grow(1.0).min_w(0.0))
            .child(cell(metrics.cpu, CPU_W, k))
            .child(cell(metrics.memory, MEMORY_W, k))
            .child(cell(metrics.age, AGE_W, k))
            .child(actions(service, s, k)),
        Layout::Stacked => col()
            .gap(6.0)
            .child(
                row()
                    .gap(COLUMN_GAP)
                    .center_y()
                    .child(who.grow(1.0))
                    .child(actions(service, s, k)),
            )
            .child(
                row()
                    .gap(COLUMN_GAP)
                    .child(endpoints(service, k).grow(1.0).min_w(0.0))
                    .child(mono(
                        format!(
                            "CPU {} · {} · up {}",
                            metrics.cpu, metrics.memory, metrics.age
                        ),
                        k,
                    )),
            ),
    };
    let selected = s.selected == Some(identity);
    row()
        .key(format!(
            "service-{}-{}",
            identity.pid,
            identity.started_at_secs.unwrap_or(0)
        ))
        .min_h(40.0)
        .px(ROW_PX)
        .py(6.0)
        .center_y()
        .radius(8.0)
        .fill(if selected { k.c.layer_02 } else { k.c.layer_01 })
        .child(content.w_full())
}

/// Inspect and Stop… in fixed slots; Stop… only where the shared policy offers it.
fn actions(service: &Service, s: &State, k: &Kit) -> El {
    let identity = service.process.identity;
    let kosse = s.snapshot.as_ref().map_or(0, |snap| snap.instance_pid);
    let inspect = button(
        "Inspect",
        format!("inspect-{}", identity.pid),
        SECONDARY,
        k,
        move |s| s.selected = Some(identity),
    );
    let stop = if present::stop_offered(service, kosse) {
        let target = service.clone();
        Button::new("Stop…")
            .variant(ButtonVariant::Danger)
            .on_press(move |s: &mut State| s.confirm(target.clone()))
            .build(k)
            .key(format!("stop-{}", identity.pid))
    } else {
        el()
    };
    row()
        .w(ACTIONS_W)
        .gap(ACTION_GAP)
        .center_y()
        .child(row().w(INSPECT_W).justify_end().child(inspect))
        .child(row().w(STOP_W).justify_end().child(stop))
}

/// What was observed on the endpoint, in words: an HTTP status is an observation, not health.
fn status(endpoint: &Endpoint) -> String {
    match &endpoint.http {
        HttpProbe::Probed { result, .. } => match result {
            ProbeResult::Response { status, .. } => format!("HTTP {status}"),
            ProbeResult::TimedOut => "Timed out".into(),
            ProbeResult::TlsFailed { .. } => "TLS failed".into(),
            ProbeResult::Refused => "Refused".into(),
            ProbeResult::Closed => "Connection closed".into(),
            ProbeResult::NotHttp => "TCP".into(),
            ProbeResult::InvalidPath => "Probe path invalid".into(),
        },
        HttpProbe::Pending => "Checking HTTP…".into(),
        HttpProbe::Skipped { .. } => "Listening".into(),
    }
}

fn endpoints(service: &Service, k: &Kit) -> El {
    let identity = service.process.identity;
    let mut lines = col().gap(2.0);
    for endpoint in &service.endpoints {
        let port = endpoint.port;
        let action = if let Some(url) = present::open_url(&endpoint.http) {
            let url = url.to_owned();
            button("Open", format!("open-{port}"), SECONDARY, k, move |s| {
                if let Err(error) = crate::opener::open(&url) {
                    s.error = Some(error);
                }
            })
        } else if matches!(endpoint.http, HttpProbe::Skipped { reason } if reason.allows_request())
        {
            button("Try HTTP", format!("probe-{port}"), QUIET, k, move |s| {
                s.send(Command::Probe(ProbeRequest { identity, port }));
            })
        } else {
            el()
        };
        lines = lines.child(
            row()
                .gap(8.0)
                .center_y()
                .min_h(28.0)
                .child(mono(format!(":{port}"), k).w(PORT_W))
                .child(
                    text(status(endpoint))
                        .font(k.ty.ui)
                        .color(k.c.text_muted)
                        .truncate()
                        .w(STATUS_W),
                )
                .child(row().w(ENDPOINT_ACTION_W).child(action)),
        );
    }
    lines
}

struct Metrics {
    cpu: String,
    memory: String,
    age: String,
}

impl Metrics {
    fn of(service: &Service, s: &State) -> Self {
        let process = &service.process;
        let cpu = match process.metrics.cpu {
            CpuUsage::Sampled { percent } => format!("{percent:.1}%"),
            CpuUsage::WarmingUp => "Warming up".into(),
            CpuUsage::Unavailable => "Unknown".into(),
        };
        let memory = process.metrics.memory_bytes.map_or_else(
            || "Unknown".into(),
            |bytes| format!("{} MB", bytes / 1_000_000),
        );
        let age = process.identity.started_at_secs.map_or_else(
            || "Unknown".into(),
            |start| {
                let now = observation(s).map_or(start, |o| o.scanned_at_ms / 1000);
                age(now.saturating_sub(start))
            },
        );
        Self { cpu, memory, age }
    }
}
