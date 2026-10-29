//! The inspector: observed process facts, then the matched profile's declared values.
use super::{
    State, display_name, observation,
    theme::{QUIET, SECONDARY, button, caption, mono, muted},
};
use crate::{present, text::printable};
use kosse_core::{model::Service, profiles::view::ProfileMatch};
use rune::{El, ScrollSpec, TextStyle, col, row, text};
use rune_kit::Kit;

/// Width of the side sheet at wide window sizes.
const INSPECTOR_W: f32 = 320.0;

fn unknown(value: Option<String>) -> String {
    value.map_or_else(|| "Unknown".into(), |value| printable(&value))
}

pub fn view(service: &Service, s: &State, k: &Kit) -> El {
    let p = &service.process;
    let mut content = col().gap(16.0).child(
        row()
            .center_y()
            .gap(8.0)
            .child(
                text(display_name(service, s))
                    .wrap_anywhere()
                    .font(TextStyle {
                        size: 19.0,
                        ..k.ty.ui_medium
                    })
                    .color(k.c.text_base)
                    .grow(1.0),
            )
            .child(button("Done", "close-inspector", SECONDARY, k, |s| {
                s.selected = None;
            })),
    );
    let facts = [
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
        ("Working directory", unknown(p.cwd.clone())),
        ("Executable", unknown(p.executable.clone())),
        ("Parent", unknown(p.parent_pid.map(|v| v.to_string()))),
        (
            "Command",
            unknown(p.command.as_ref().map(|c| c.args.join(" "))),
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
                        printable(&e.bind_addresses.join(", ")),
                        e.exposure
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ];
    for (title, value) in facts {
        content = content.child(
            col()
                .gap(4.0)
                .child(caption(title, k))
                .child(mono(value, k).wrap_anywhere().selectable(true)),
        );
    }
    content = content.child(profile(service, s, k));
    col()
        .key("inspector")
        .w(INSPECTOR_W)
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

fn profile(service: &Service, s: &State, k: &Kit) -> El {
    let mut content = col().gap(8.0);
    let entry = observation(s).and_then(|o| present::configured(service, o));
    if let Some(entry) = entry {
        content = content.child(caption("Profile", k)).child(
            muted(
                format!(
                    "{} · name from {}",
                    printable(&entry.id),
                    entry
                        .sources
                        .name
                        .map_or("observation".into(), |source| source.to_string())
                ),
                k,
            )
            .wrap(),
        );
        if let Some(protocol) = entry.protocol {
            content = content.child(muted(format!("Declared protocol: {protocol:?}"), k));
        }
        for link in &entry.links {
            // Core validated the URL as http(s); the opener refuses anything else again.
            let url = link.url.clone();
            content = content.child(button(
                &printable(&format!("{} · {} (declared)", link.label, link.host)),
                format!("link-{}", link.label),
                QUIET,
                k,
                move |s| {
                    if let Err(error) = crate::opener::open(&url) {
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
                    printable(&services.join(", "))
                ),
                k,
            )
            .wrap(),
        );
    }
    content
}
