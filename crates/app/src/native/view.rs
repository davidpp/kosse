use super::{State, client::Command};
use kosse_core::profiles::view::{ConfiguredService, ProfileMatch};
use kosse_core::{
    action::ActionResult,
    contract::ProbeRequest,
    model::{CpuUsage, HttpProbe, ProbeResult, ProbeSkip, Service},
};
use rune::{Color, Cx, El, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, PaletteSeed, TextInput};

fn configured<'a>(service: &Service, s: &'a State) -> Option<&'a ConfiguredService> {
    let ProfileMatch::Matched { service: id } = service.profile.as_ref()? else {
        return None;
    };
    s.snapshot
        .as_ref()?
        .observation
        .as_ref()?
        .profiles
        .iter()
        .find(|p| Some(&p.root) == service.project.as_ref())?
        .services
        .iter()
        .find(|entry| &entry.id == id)
}
fn display_name(service: &Service, s: &State) -> String {
    configured(service, s)
        .and_then(|entry| entry.name.clone())
        .unwrap_or_else(|| name(service))
}
fn age(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    } else {
        format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600)
    }
}

fn kit(dark: bool) -> Kit {
    let rgb = |r: u8, g: u8, b: u8| Color {
        r: rune::color::srgb_decode(f32::from(r) / 255.0),
        g: rune::color::srgb_decode(f32::from(g) / 255.0),
        b: rune::color::srgb_decode(f32::from(b) / 255.0),
        a: 1.0,
    };
    let mut kit = Kit::from_palette(PaletteSeed {
        bg: if dark {
            rgb(24, 24, 26)
        } else {
            rgb(247, 247, 245)
        },
        fg: if dark {
            rgb(237, 237, 235)
        } else {
            rgb(35, 35, 38)
        },
        accent: if dark {
            rgb(145, 179, 255)
        } else {
            rgb(0, 61, 165)
        },
    });
    kit.c.bg_contrast = if dark {
        rgb(145, 179, 255)
    } else {
        rgb(0, 61, 165)
    };
    kit.c.text_contrast = if dark {
        rgb(18, 28, 48)
    } else {
        rgb(255, 255, 255)
    };
    kit.c.icon_contrast = kit.c.text_contrast;
    kit
}
fn label(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.ui).color(k.c.text_base)
}
fn muted(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.ui).color(k.c.text_muted)
}
fn mono(value: impl Into<String>, k: &Kit) -> El {
    text(value)
        .font(TextStyle {
            family: rune::Family::SystemMono,
            size: 12.0,
            ..k.ty.ui
        })
        .color(k.c.text_muted)
}
fn button(value: &str, key: &str, k: &Kit, action: impl Fn(&mut State) + 'static) -> El {
    Button::new(value)
        .variant(ButtonVariant::Ghost)
        .on_press(action)
        .build(k)
        .key(key)
}
fn name(service: &Service) -> String {
    service
        .process
        .name
        .clone()
        .or_else(|| service.package.as_ref().map(|p| p.name.clone()))
        .unwrap_or_else(|| format!("PID {}", service.process.identity.pid))
}
fn noise(service: &Service) -> bool {
    service.project.is_none()
        || service.endpoints.iter().any(|e| {
            matches!(
                e.http,
                HttpProbe::Skipped {
                    reason: ProbeSkip::OtherUser
                }
            )
        })
}
fn matches(service: &Service, query: &str, s: &State) -> bool {
    let observation = s
        .snapshot
        .as_ref()
        .and_then(|snap| snap.observation.as_ref());
    let project_name = observation
        .and_then(|o| {
            o.profiles
                .iter()
                .find(|p| Some(&p.root) == service.project.as_ref())
        })
        .and_then(|p| p.name.as_deref())
        .unwrap_or("");
    let branch = observation
        .and_then(|o| {
            o.projects
                .iter()
                .find(|p| Some(&p.root) == service.project.as_ref())
        })
        .and_then(|p| p.git.as_ref())
        .map_or(String::new(), |git| match &git.head {
            kosse_core::model::GitHead::Branch { name } => name.clone(),
            kosse_core::model::GitHead::Detached { commit } => commit.clone(),
            kosse_core::model::GitHead::Unknown => String::new(),
        });
    let command = service
        .process
        .command
        .as_ref()
        .map_or_else(String::new, |c| c.args.join(" "));
    let haystack = format!(
        "{project_name} {branch} {command} {} {} {} {} {}",
        display_name(service, s),
        name(service),
        service.project.as_deref().unwrap_or(""),
        service.package.as_ref().map_or("", |p| &p.path),
        service
            .endpoints
            .iter()
            .map(|e| e.port.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|term| haystack.contains(term))
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let k = kit(s.theme.unwrap_or_else(|| cx.system_dark()));
    let narrow = cx.layout_size("kosse").is_some_and(|size| size[0] < 950.0);
    let mut layers = Vec::new();
    let (header, toolbar) = navigation(s, &k);
    let list = project_list(s, &k, narrow);
    let mut body = row().key("body").grow(1.0).min_h(0.0).child(
        list.scroll(ScrollSpec {
            stick_bottom: false,
        })
        .grow(1.0)
        .min_w(0.0),
    );
    if let Some(service) = s
        .snapshot
        .as_ref()
        .and_then(|snap| snap.observation.as_ref())
        .and_then(|o| {
            o.services
                .iter()
                .find(|service| Some(service.process.identity) == s.selected)
        })
    {
        if narrow {
            body = row()
                .grow(1.0)
                .min_h(0.0)
                .child(inspect(service, s, &k).grow(1.0));
        } else {
            body = body.child(inspect(service, s, &k));
        }
    }
    if let Some(dialog) = &s.confirmation {
        confirmation(dialog, &k, &mut layers);
    }
    super::customize::view(
        s,
        &k,
        &mut layers,
        cx.layout_size("kosse").map_or(780.0, |size| size[1]),
    );
    rune_kit::root(
        col()
            .key("kosse")
            .w_full()
            .h_full()
            .fill(k.c.bg_base)
            .child(header)
            .child(toolbar)
            .child(el().h(1.0).w_full().fill(k.c.border_weak))
            .child(body)
            .child(
                row()
                    .h(32.0)
                    .px(20.0)
                    .center_y()
                    .child(status_notes(s, &k))
                    .child(el().grow(1.0))
                    .child(mono("/ Search    Esc Close", &k)),
            ),
        layers,
    )
}

fn service_row(service: &Service, s: &State, k: &Kit, compact: bool) -> El {
    let identity = service.process.identity;
    let selected = s.selected == Some(identity);
    let endpoints = endpoints(service, k);
    let mut actions = row().gap(4.0).child(button(
        "Inspect",
        &format!("inspect-{}", identity.pid),
        k,
        move |s| s.selected = Some(identity),
    ));
    if identity.started_at_secs.is_some()
        && identity.pid > 1
        && s.snapshot
            .as_ref()
            .is_some_and(|snap| snap.instance_pid != identity.pid)
        && !service.endpoints.iter().any(|e| {
            matches!(
                e.http,
                HttpProbe::Skipped {
                    reason: ProbeSkip::OtherUser
                }
            )
        })
    {
        let target = service.clone();
        actions = actions.child(
            Button::new("Stop…")
                .variant(ButtonVariant::Danger)
                .on_press(move |s: &mut State| s.confirm(target.clone()))
                .build(k)
                .key(format!("stop-{}", identity.pid)),
        );
    }
    let identity_column = col()
        .w(if compact { 120.0 } else { 160.0 })
        .gap(5.0)
        .child(
            text(display_name(service, s))
                .truncate()
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(
            mono(
                service
                    .package
                    .as_ref()
                    .map_or("Unattributed", |p| p.path.as_str()),
                k,
            )
            .truncate(),
        );
    let metrics = metrics(service, s, k);
    let content = if compact {
        col()
            .gap(8.0)
            .child(
                row()
                    .gap(12.0)
                    .child(identity_column)
                    .child(el().grow(1.0))
                    .child(actions),
            )
            .child(row().gap(12.0).child(endpoints).child(metrics))
    } else {
        row()
            .gap(16.0)
            .center_y()
            .child(identity_column)
            .child(endpoints)
            .child(metrics)
            .child(actions)
    };
    col()
        .key(format!(
            "service-{}-{}",
            identity.pid,
            identity.started_at_secs.unwrap_or(0)
        ))
        .min_h(72.0)
        .px(14.0)
        .py(10.0)
        .gap(16.0)
        .center_y()
        .radius(8.0)
        .fill(if selected { k.c.layer_02 } else { k.c.layer_01 })
        .child(content.w_full())
}

fn inspect(service: &Service, s: &State, k: &Kit) -> El {
    let p = &service.process;
    let mut content = col().gap(18.0).child(
        row()
            .center_y()
            .child(
                text(display_name(service, s))
                    .font(TextStyle {
                        size: 19.0,
                        ..k.ty.ui_medium
                    })
                    .color(k.c.text_base)
                    .grow(1.0),
            )
            .child(button("Done", "close-inspector", k, |s| s.selected = None)),
    );
    for (title, value) in [
        (
            "Process",
            format!(
                "PID {} · started {}",
                p.identity.pid,
                p.identity
                    .started_at_secs
                    .map_or_else(|| "unknown".into(), |v| v.to_string())
            ),
        ),
        (
            "Working directory",
            p.cwd.clone().unwrap_or_else(|| "Unknown".into()),
        ),
        (
            "Executable",
            p.executable.clone().unwrap_or_else(|| "Unknown".into()),
        ),
        (
            "Parent",
            p.parent_pid
                .map_or_else(|| "Unknown".into(), |v| v.to_string()),
        ),
        (
            "Command",
            p.command
                .as_ref()
                .map_or_else(|| "Unknown".into(), |c| c.args.join(" ")),
        ),
        (
            "Endpoints",
            service
                .endpoints
                .iter()
                .map(|e| {
                    format!(
                        "{} · {} · {:?}",
                        e.port,
                        e.bind_addresses.join(", "),
                        e.exposure
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ] {
        content = content.child(
            col()
                .gap(6.0)
                .child(label(title, k))
                .child(mono(value, k).wrap_anywhere().selectable(true)),
        );
    }
    content = content.child(profile_details(service, s, k));
    col()
        .key("inspector")
        .w(320.0)
        .h_full()
        .fill(k.c.layer_01)
        .padding(20.0)
        .child(
            content
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0),
        )
}

fn confirmation(dialog: &super::Confirmation, k: &Kit, layers: &mut Vec<El>) {
    let heading = if dialog.busy {
        "Waiting for the process…"
    } else if dialog.force {
        "Force this process to stop?"
    } else if dialog.result.is_some() {
        "Stop result"
    } else {
        "Stop this process?"
    };
    let description = format!(
        "{} · PID {}\n{}\nAll affected ports: {}",
        name(&dialog.service),
        dialog.request.identity.pid,
        dialog
            .service
            .project
            .as_deref()
            .unwrap_or("No attributed project"),
        dialog
            .request
            .ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut body = col()
        .padding(20.0)
        .gap(16.0)
        .child(label(description, k).wrap_anywhere());
    if let Some(result) = &dialog.result {
        body = body.child(label(result_text(result), k).wrap());
    }
    if let Some(error) = &dialog.error {
        body = body.child(label(error, k).wrap());
    }
    let controls = confirmation_controls(dialog, k);
    body = body.child(controls);
    Dialog::new("stop", heading)
        .open(true)
        .fit()
        .width(510.0)
        .click_outside(false)
        .on_close(|s: &mut State| {
            if s.confirmation.as_ref().is_some_and(|d| !d.busy) {
                s.confirmation = None;
            }
        })
        .child(body)
        .build(k, layers);
}

fn result_text(result: &ActionResult) -> String {
    match result {
        ActionResult::Exited { shared, .. } => if shared.is_empty() { "Process exited. Its ports are free.".into() } else { "Process exited. Other processes still share these ports.".into() },
        ActionResult::StillRunning { .. } => "Still running after graceful termination. Force requires another confirmation.".into(),
        ActionResult::Respawned { .. } => "The process exited, but its supervisor started a replacement. The parent was not stopped.".into(),
        ActionResult::EndpointTaken { .. } => "The process exited. Another process now owns an affected port.".into(),
        ActionResult::Stale { .. } => "The process or ports changed. Nothing was sent. Refresh and confirm again.".into(),
        ActionResult::ForceNotOffered => "The force offer expired or is unavailable. Nothing was sent.".into(),
        ActionResult::Refused { .. } => "Kossé cannot stop this process. Nothing was sent.".into(),
        ActionResult::PermissionDenied { .. } => "The operating system refused permission.".into(),
        ActionResult::ObservationFailed { reason, .. } => format!("Signal sent; the outcome could not be observed: {reason}"),
        ActionResult::SignalFailed { reason, .. } => format!("No signal was sent: {reason}"),
        ActionResult::Internal { reason } => format!("Outcome unknown. Refresh: {reason}"),
    }
}

pub fn on_key(s: &mut State, key: &KeyEvent, _tx: &mut Tx) -> bool {
    if matches!(&key.key, Key::Text(text) if text == "/")
        && !s.search.focused
        && s.editor.is_none()
        && s.confirmation.is_none()
    {
        s.search.focused = true;
        return true;
    }
    if key.key == Key::Escape {
        if s.confirmation.as_ref().is_some_and(|d| d.busy)
            || s.editor.as_ref().is_some_and(|e| e.busy)
        {
            return true;
        }
        s.confirmation = None;
        s.editor = None;
        s.selected = None;
        s.search.replace_text("");
        return true;
    }
    false
}

fn profile_details(service: &Service, s: &State, k: &Kit) -> El {
    let mut content = col().gap(12.0);
    if let Some(entry) = configured(service, s) {
        content = content.child(label(
            format!(
                "Profile: {} · name from {}",
                entry.id,
                entry
                    .sources
                    .name
                    .map_or("observation".into(), |source| source.to_string())
            ),
            k,
        ));
        if let Some(protocol) = entry.protocol {
            content = content.child(muted(format!("Declared protocol: {protocol:?}"), k));
        }
        for link in &entry.links {
            let url = link.url.clone();
            content = content.child(button(
                &format!("{} · {} (declared)", link.label, link.host),
                &format!("link-{}", link.label),
                k,
                move |s| {
                    if (url.starts_with("https://") || url.starts_with("http://"))
                        && let Err(error) = crate::opener::open(&url)
                    {
                        s.error = Some(error);
                    }
                },
            ));
        }
    } else if let Some(ProfileMatch::Ambiguous { services }) = &service.profile {
        content = content.child(
            muted(
                format!(
                    "Ambiguous profile matches: {}. No override applied.",
                    services.join(", ")
                ),
                k,
            )
            .wrap(),
        );
    }
    content
}

fn endpoints(service: &Service, k: &Kit) -> El {
    let identity = service.process.identity;
    let mut endpoints = col().gap(6.0).grow(1.0).min_w(0.0);
    for endpoint in &service.endpoints {
        let (status, url) = match &endpoint.http {
            HttpProbe::Probed {
                result: ProbeResult::Response { status, url, .. },
                ..
            } => (format!("HTTP {status}"), Some(url.clone())),
            HttpProbe::Probed { result, .. } => (
                match result {
                    ProbeResult::TimedOut => "Timed out",
                    ProbeResult::TlsFailed { .. } => "TLS failed",
                    ProbeResult::Refused => "Refused",
                    ProbeResult::Closed => "Connection closed",
                    ProbeResult::NotHttp => "TCP",
                    _ => "Not checked",
                }
                .into(),
                None,
            ),
            HttpProbe::Pending => ("Checking HTTP…".into(), None),
            HttpProbe::Skipped { .. } => ("Listening".into(), None),
        };
        let mut line = row()
            .gap(10.0)
            .center_y()
            .child(mono(format!(":{}", endpoint.port), k).w(62.0))
            .child(muted(status, k).grow(1.0));
        if let Some(url) = url {
            line = line.child(button(
                "Open",
                &format!("open-{}", endpoint.port),
                k,
                move |s| {
                    if let Err(error) = crate::opener::open(&url) {
                        s.error = Some(error);
                    }
                },
            ));
        } else if matches!(endpoint.http, HttpProbe::Skipped { reason } if reason.allows_request())
        {
            let port = endpoint.port;
            line = line.child(button("Try HTTP", &format!("probe-{port}"), k, move |s| {
                s.send(Command::Probe(ProbeRequest { identity, port }));
            }));
        }
        endpoints = endpoints.child(line);
    }
    endpoints
}

fn metrics(service: &Service, s: &State, k: &Kit) -> El {
    let identity = service.process.identity;
    let cpu = match service.process.metrics.cpu {
        CpuUsage::Sampled { percent } => format!("{percent:.1}%"),
        CpuUsage::WarmingUp => "Warming up".into(),
        CpuUsage::Unavailable => "Unknown".into(),
    };
    let memory = service.process.metrics.memory_bytes.map_or_else(
        || "Unknown".into(),
        |bytes| format!("{} MB", bytes / 1_000_000),
    );
    let uptime = identity.started_at_secs.map(|start| {
        s.snapshot
            .as_ref()
            .and_then(|snap| snap.observation.as_ref())
            .map_or(start, |o| o.scanned_at_ms / 1000)
            .saturating_sub(start)
    });
    col()
        .w(145.0)
        .gap(5.0)
        .child(mono(format!("{cpu} CPU · {memory}"), k))
        .child(mono(uptime.map_or_else(|| "Age unknown".into(), age), k))
}

fn confirmation_controls(dialog: &super::Confirmation, k: &Kit) -> El {
    let mut controls = row().gap(8.0).child(
        Button::new(if dialog.result.is_some() {
            "Done"
        } else {
            "Cancel"
        })
        .disabled(dialog.busy)
        .on_press(|s: &mut State| s.confirmation = None)
        .build(k)
        .key("cancel-stop"),
    );
    if dialog.result.is_none() {
        controls = controls.child(
            Button::new(if dialog.force {
                "Force stop"
            } else {
                "Stop process"
            })
            .variant(ButtonVariant::Danger)
            .disabled(dialog.busy)
            .on_press(|s: &mut State| {
                let request = s.confirmation.as_mut().map(|d| {
                    d.busy = true;
                    d.error = None;
                    (d.request.clone(), d.force)
                });
                if let Some((request, force)) = request {
                    s.send(Command::Action(request, force));
                }
            })
            .build(k)
            .key("confirm-stop"),
        );
    } else if matches!(
        dialog.result,
        Some(ActionResult::StillRunning {
            force_until_ms: Some(_),
            ..
        })
    ) {
        controls = controls.child(
            Button::new("Force…")
                .variant(ButtonVariant::Danger)
                .on_press(|s: &mut State| {
                    if let Some(d) = &mut s.confirmation {
                        d.force = true;
                        d.result = None;
                    }
                })
                .build(k)
                .key("offer-force"),
        );
    }
    controls
}

fn project_list(s: &State, k: &Kit, narrow: bool) -> El {
    let mut list = col().key("projects").gap(24.0).padding(20.0).w_full();
    if let Some(error) = &s.connection_error {
        list = list.child(label(error, k).wrap());
    }
    if let Some(error) = &s.error {
        list = list.child(
            row()
                .gap(8.0)
                .child(label(error, k).grow(1.0))
                .child(button("Dismiss", "dismiss-error", k, |s| s.error = None)),
        );
    }
    match s
        .snapshot
        .as_ref()
        .and_then(|snap| snap.observation.as_ref())
    {
        None => {
            list = list.child(label("Discovering local services…", k));
        }
        Some(observation) => {
            if let Some(failure) = s
                .snapshot
                .as_ref()
                .and_then(|snap| snap.scanner.failure.as_ref())
            {
                list = list.child(label(
                    format!("Showing the last good observation: {failure}"),
                    k,
                ));
            }
            if s.notes {
                for warning in &observation.warnings {
                    list = list.child(muted(warning.to_string(), k).wrap());
                }
            }
            let mut roots: Vec<Option<&str>> = observation
                .projects
                .iter()
                .map(|p| Some(p.root.as_str()))
                .collect();
            for profile in &observation.profiles {
                if !roots.contains(&Some(profile.root.as_str())) {
                    roots.push(Some(&profile.root));
                }
            }
            roots.push(None);
            let mut count = 0;
            for root in roots {
                if let Some((group, total)) = project_group(s, observation, root, k, narrow) {
                    list = list.child(group);
                    count += total;
                }
            }
            for (service, seen) in &s.ended {
                if (s.all || !noise(service)) && matches(service, &s.search.text, s) {
                    list = list.child(
                        muted(
                            format!(
                                "{} · ended · last observed {} ago · ports {}",
                                name(service),
                                age(observation.scanned_at_ms.saturating_sub(*seen) / 1000),
                                service
                                    .endpoints
                                    .iter()
                                    .map(|e| e.port.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            k,
                        )
                        .wrap(),
                    );
                }
            }
            if count == 0 {
                list = list.child(label(if s.search.text.is_empty() { "No project services are listening. Start a development server, or show all listeners." } else { "No matching services. Try another project, process or port." }, k));
            }
        }
    }
    list
}

fn project_group(
    s: &State,
    observation: &kosse_core::model::Observation,
    root: Option<&str>,
    k: &Kit,
    narrow: bool,
) -> Option<(El, usize)> {
    let services: Vec<_> = observation
        .services
        .iter()
        .filter(|service| {
            service.project.as_deref() == root
                && (s.all || !noise(service))
                && matches(service, &s.search.text, s)
        })
        .collect();
    let metadata = observation
        .profiles
        .iter()
        .find(|p| Some(p.root.as_str()) == root);
    let configured_only: Vec<_> = metadata
        .into_iter()
        .flat_map(|m| &m.services)
        .filter(|e| e.matched.is_empty())
        .collect();
    let profile_matches = metadata.is_some_and(|m| {
        format!("{} {}", m.root, m.name.as_deref().unwrap_or(""))
            .to_lowercase()
            .contains(&s.search.text.to_lowercase())
    });
    if services.is_empty()
        && !(profile_matches
            && (metadata.is_some_and(|m| !m.observed) || !configured_only.is_empty()))
    {
        return None;
    }
    let count = services.len();
    let mut group = group_heading(s, observation, root, count, k);
    if let Some(metadata) = metadata {
        group = group.child(
            mono(
                format!(
                    "{} · {}",
                    metadata.root,
                    metadata
                        .sources
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" + ")
                ),
                k,
            )
            .truncate(),
        );
    }
    if profile_matches {
        for entry in configured_only {
            group = group.child(muted(
                format!(
                    "{} · {}",
                    entry.name.as_deref().unwrap_or(&entry.id),
                    if entry.disabled {
                        "Disabled in profile"
                    } else {
                        "Configured, not observed"
                    }
                ),
                k,
            ));
        }
    }
    for service in services {
        group = group.child(service_row(service, s, k, narrow || s.selected.is_some()));
    }
    Some((group, count))
}

fn group_heading(
    _s: &State,
    observation: &kosse_core::model::Observation,
    root: Option<&str>,
    count: usize,
    k: &Kit,
) -> El {
    let project = observation
        .projects
        .iter()
        .find(|p| Some(p.root.as_str()) == root);
    let title = observation
        .profiles
        .iter()
        .find(|m| Some(m.root.as_str()) == root)
        .and_then(|m| m.name.as_deref())
        .unwrap_or_else(|| project.map_or("Other listeners", |p| p.name.as_str()));
    let branch = project
        .and_then(|p| p.git.as_ref())
        .map(|git| match &git.head {
            kosse_core::model::GitHead::Branch { name } => name.clone(),
            kosse_core::model::GitHead::Detached { commit } => commit.clone(),
            kosse_core::model::GitHead::Unknown => "Unknown branch".into(),
        })
        .unwrap_or_default();
    let mut heading = row()
        .center_y()
        .gap(12.0)
        .child(
            text(title)
                .font(TextStyle {
                    size: 17.0,
                    ..k.ty.ui_medium
                })
                .color(k.c.text_base),
        )
        .child(mono(branch, k))
        .child(el().grow(1.0))
        .child(muted(format!("{count} listening"), k));
    if let Some(root) = root {
        let root = root.to_owned();
        heading = heading.child(button(
            "Customize…",
            &format!("customize-{root}"),
            k,
            move |s| {
                if let Some(observation) = s
                    .snapshot
                    .as_ref()
                    .and_then(|snap| snap.observation.as_ref())
                {
                    s.editor = Some(super::customize::Editor::new(root.clone(), observation));
                }
            },
        ));
    }
    col()
        .key(root.unwrap_or("other-listeners"))
        .gap(8.0)
        .child(heading)
}

fn navigation(s: &State, k: &Kit) -> (El, El) {
    let header = row()
        .key("titlebar")
        .h(50.0)
        .pl(90.0)
        .pr(20.0)
        .gap(12.0)
        .center_y()
        .window_drag()
        .child(
            text("Kossé")
                .font(TextStyle {
                    family: rune::Family::Named("Kibernetyk Mono"),
                    weight: 600.0,
                    size: 19.0,
                    ..k.ty.ui_medium
                })
                .color(k.c.text_base),
        )
        .child(el().grow(1.0))
        .child(muted(
            if s.connected {
                "Live on this Mac"
            } else {
                "Connecting…"
            },
            k,
        ))
        .child(button("Refresh", "refresh", k, |s| {
            s.send(Command::Refresh);
        }));
    let search = TextInput::new(&s.search)
        .placeholder("Find a project, process or port…")
        .on_edit(|s: &mut State| &mut s.search)
        .on_action(|s: &mut State, _| s.search.replace_text(""))
        .clear_button()
        .build(k)
        .key("search")
        .grow(1.0);
    let toolbar = row()
        .px(20.0)
        .pb(16.0)
        .gap(12.0)
        .center_y()
        .child(search)
        .child(button(
            if s.all {
                "All listeners"
            } else {
                "Projects only"
            },
            "all-listeners",
            k,
            |s| {
                s.all = !s.all;
                s.send(Command::Preferences(s.all));
            },
        ));
    (header, toolbar)
}

fn status_notes(s: &State, k: &Kit) -> El {
    let observation = s
        .snapshot
        .as_ref()
        .and_then(|snap| snap.observation.as_ref());
    let count = observation.map_or(0, |o| o.warnings.len());
    if count == 0 {
        return muted("TCP listeners", k);
    }
    button(
        &if s.notes {
            format!("Hide {count} observation notes")
        } else {
            format!("{count} observation notes…")
        },
        "observation-notes",
        k,
        |s| s.notes = !s.notes,
    )
}
