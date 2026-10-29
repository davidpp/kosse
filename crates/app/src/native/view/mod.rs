//! The dashboard: a title row, search and scope, project groups of service rows, the inspector,
//! the Stop confirmation and the footer. Names, branches and offered actions come from
//! [`crate::present`], the same rules the tray menu uses.
mod confirm;
mod inspect;
mod list;
mod row;
mod theme;

use super::{State, client::Command};
use crate::{present, text::printable};
use kosse_core::model::{Observation, Service};
use rune::{Cx, El, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, el, row, text};
use rune_kit::{ControlSize, Kit, Switch, TextInput};
use theme::{QUIET, SECONDARY, button, kit, mono, muted};

/// Below this window width (or with the inspector open) rows stack onto two lines and the
/// inspector replaces the list.
const NARROW_BELOW: f32 = 980.0;
/// Window height assumed before the first layout.
const DEFAULT_HEIGHT: f32 = 780.0;
/// Leading space in the title row for the window controls.
const TRAFFIC_LIGHTS: f32 = 90.0;

fn observation(s: &State) -> Option<&Observation> {
    s.snapshot.as_ref()?.observation.as_ref()
}

fn display_name(service: &Service, s: &State) -> String {
    observation(s).map_or_else(
        || printable(service.process.name.as_deref().unwrap_or("")),
        |o| present::service_name(service, o),
    )
}

/// `3000, 3001`.
fn port_list(ports: impl IntoIterator<Item = u16>) -> String {
    ports
        .into_iter()
        .map(|port| port.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let k = kit(s.theme.unwrap_or_else(|| cx.system_dark()));
    let size = cx.layout_size("kosse");
    let narrow = size.is_some_and(|size| size[0] < NARROW_BELOW);
    let mut layers = Vec::new();
    let selected = observation(s).and_then(|o| {
        o.services
            .iter()
            .find(|service| Some(service.process.identity) == s.selected)
    });
    let list = list::projects(s, &k, narrow || selected.is_some())
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .grow(1.0)
        .min_w(0.0);
    let body = match selected {
        Some(service) if narrow => row().child(inspect::view(service, s, &k).grow(1.0)),
        Some(service) => row().child(list).child(inspect::view(service, s, &k)),
        None => row().child(list),
    };
    if let Some(dialog) = &s.confirmation {
        confirm::view(dialog, s, &k, &mut layers);
    }
    super::customize::view(
        s,
        &k,
        &mut layers,
        size.map_or(DEFAULT_HEIGHT, |size| size[1]),
    );
    rune_kit::root(
        col()
            .key("kosse")
            .w_full()
            .h_full()
            .fill(k.c.bg_base)
            .child(header(s, &k))
            .child(toolbar(s, &k))
            .child(el().h(1.0).w_full().fill(k.c.border_weak))
            .child(body.key("body").grow(1.0).min_h(0.0))
            .child(el().h(1.0).w_full().fill(k.c.border_weak))
            .child(footer(s, &k)),
        layers,
    )
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

/// The connection state in words, with a dot that is never the only signal.
fn connection(s: &State, k: &Kit) -> El {
    let stale = s
        .snapshot
        .as_ref()
        .is_some_and(|snap| snap.scanner.failure.is_some());
    let (words, dot) = match (s.connected, stale) {
        (true, false) => ("Live", k.c.success.fg),
        (true, true) => ("Live · last scan failed", k.c.warning.fg),
        (false, _) if s.snapshot.is_some() => ("Reconnecting…", k.c.warning.fg),
        (false, _) => ("Connecting…", k.c.text_faint),
    };
    row()
        .key("connection")
        .gap(6.0)
        .center_y()
        .child(el().w(8.0).h(8.0).radius(4.0).fill(dot))
        .child(muted(words, k))
}

fn header(s: &State, k: &Kit) -> El {
    row()
        .key("titlebar")
        .h(50.0)
        .pl(TRAFFIC_LIGHTS)
        .pr(20.0)
        .gap(16.0)
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
        .child(connection(s, k))
        .child(button("Refresh", "refresh", SECONDARY, k, |s| {
            s.send(Command::Refresh);
        }))
}

fn toolbar(s: &State, k: &Kit) -> El {
    let search = TextInput::new(&s.search)
        .placeholder("Find a project, process or port…")
        .on_edit(|s: &mut State| &mut s.search)
        .on_action(|s: &mut State, _| s.search.replace_text(""))
        .clear_button()
        .build(k)
        .key("search")
        .grow(1.0);
    let scope = Switch::new(s.all)
        .label("All listeners")
        .control_size(ControlSize::Small)
        .on_toggle(|s: &mut State| {
            s.all = !s.all;
            s.send(Command::Preferences(s.all));
        })
        .build(k)
        .key("all-listeners");
    row()
        .px(20.0)
        .pb(12.0)
        .gap(16.0)
        .center_y()
        .child(search)
        .child(scope)
}

fn footer(s: &State, k: &Kit) -> El {
    let count = observation(s).map_or(0, |o| o.warnings.len());
    let notes = match count {
        0 => muted("No scan warnings", k),
        _ if s.notes => button("Hide scan warnings", "observation-notes", QUIET, k, |s| {
            s.notes = false;
        }),
        1 => button("1 scan warning", "observation-notes", QUIET, k, |s| {
            s.notes = true;
        }),
        count => button(
            &format!("{count} scan warnings"),
            "observation-notes",
            QUIET,
            k,
            |s| s.notes = true,
        ),
    };
    row()
        .h(32.0)
        .px(20.0)
        .center_y()
        .child(notes)
        .child(el().grow(1.0))
        .child(mono("/ Search   Esc Close", k))
}
