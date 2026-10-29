use super::{LinkEditor, ServiceEditor};
use crate::native::State;
use kosse_core::profiles::{document::DeclaredProtocol, view::ProfileSource};
use rune::{El, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Kit, TextField};

#[derive(Clone, Copy)]
pub(super) enum Field {
    Id,
    Name,
    Cwd,
    Port,
    Probe,
    LinkLabel(usize),
    LinkUrl(usize),
}
fn edit(s: &mut State, index: usize, field: Field) -> &mut TextEdit {
    let Some(entry) = s.editor.as_mut().and_then(|e| e.services.get_mut(index)) else {
        return &mut s.orphan_edit;
    };
    match field {
        Field::Id => &mut entry.id,
        Field::Name => &mut entry.name,
        Field::Cwd => &mut entry.cwd,
        Field::Port => &mut entry.port,
        Field::Probe => &mut entry.probe,
        Field::LinkLabel(i) => entry
            .links
            .get_mut(i)
            .map_or(&mut s.orphan_edit, |l| &mut l.label),
        Field::LinkUrl(i) => entry
            .links
            .get_mut(i)
            .map_or(&mut s.orphan_edit, |l| &mut l.url),
    }
}
pub(super) fn hint(value: impl Into<String>, k: &Kit) -> El {
    text(value).wrap().font(k.ty.ui).color(k.c.text_muted)
}
fn field(
    title: &str,
    value: &TextEdit,
    placeholder: &str,
    key: &str,
    index: usize,
    which: Field,
    k: &Kit,
) -> El {
    TextField::new(title, value)
        .placeholder(placeholder)
        .on_edit(move |s: &mut State| edit(s, index, which))
        .build(k)
        .key(key)
        .w_full()
        .min_w(0.0)
}
pub(super) fn identity(entry: &ServiceEditor, index: usize, k: &Kit) -> El {
    let inherited = entry
        .inherited
        .as_ref()
        .filter(|e| e.sources.name == Some(ProfileSource::Repository))
        .and_then(|e| e.name.as_deref());
    col()
        .gap(14.0)
        .child(
            TextField::new("Service ID", &entry.id)
                .placeholder("e.g. web, api, database")
                .disabled(entry.inherited.is_some())
                .on_edit(move |s: &mut State| edit(s, index, Field::Id))
                .build(k)
                .key("service-id"),
        )
        .child(hint(
            "The same ID shares settings with a service in kosse.json.",
            k,
        ))
        .child(field(
            "Display name",
            &entry.name,
            inherited.unwrap_or("Use the process name"),
            "service-name",
            index,
            Field::Name,
            k,
        ))
        .child(hint(
            inherited.map_or_else(
                || "Blank inherits the repository name, or uses the process name.".into(),
                |name| format!("Repository name: {name}. Leave blank to inherit it."),
            ),
            k,
        ))
}
pub(super) fn matching(entry: &ServiceEditor, index: usize, k: &Kit) -> El {
    let mut body = col().gap(12.0).child(
        Checkbox::new(entry.match_override)
            .label("Set a local matching rule")
            .on_toggle(move |s: &mut State| {
                if let Some(e) = s.editor.as_mut().and_then(|e| e.services.get_mut(index)) {
                    e.match_override = !e.match_override;
                }
            })
            .build(k)
            .key("match-override"),
    );
    if entry.match_override {
        body = body
            .child(field(
                "Directory within this project",
                &entry.cwd,
                "apps/web or .",
                "service-cwd",
                index,
                Field::Cwd,
                k,
            ))
            .child(field(
                "Listening port",
                &entry.port,
                "e.g. 5173",
                "service-port",
                index,
                Field::Port,
                k,
            ))
            .child(hint(
                "Set one or both. When both are set, both must match. Rules stay within this \
                 project.",
                k,
            ));
    } else {
        body = body.child(hint(
            entry
                .inherited
                .as_ref()
                .filter(|e| e.sources.matcher == Some(ProfileSource::Repository))
                .and_then(|e| e.matcher.as_ref())
                .map_or_else(
                    || {
                        "Use the repository rule when present; otherwise keep this service \
                         declared without matching a process."
                            .into()
                    },
                    |m| {
                        format!(
                            "Inherited rule · directory {} · port {}",
                            m.cwd.as_deref().unwrap_or("any"),
                            m.port.map_or_else(|| "any".into(), |p| p.to_string())
                        )
                    },
                ),
            k,
        ));
    }
    body
}
pub(super) fn protocol(entry: &ServiceEditor, index: usize, k: &Kit) -> El {
    let mut choices = row().gap(4.0);
    for (name, value) in [
        ("Inherit", None),
        ("HTTP", Some(DeclaredProtocol::Http)),
        ("HTTPS", Some(DeclaredProtocol::Https)),
        ("TCP", Some(DeclaredProtocol::Tcp)),
    ] {
        choices = choices.child(
            Button::new(name)
                .variant(if entry.protocol == value {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::Ghost
                })
                .on_press(move |s: &mut State| {
                    if let Some(e) = s.editor.as_mut().and_then(|e| e.services.get_mut(index)) {
                        e.protocol = value;
                    }
                })
                .build(k)
                .key(format!("protocol-{name}")),
        );
    }
    col()
        .gap(10.0)
        .child(hint("Protocol hint", k))
        .child(choices)
        .child(hint(
            "HTTP and HTTPS allow checks of this service on loopback. A hint is not a health \
             result.",
            k,
        ))
        .child(field(
            "HTTP probe path",
            &entry.probe,
            "/",
            "service-probe",
            index,
            Field::Probe,
            k,
        ))
}
pub(super) fn links(entry: &ServiceEditor, index: usize, k: &Kit) -> El {
    let mut body = col().gap(12.0).child(
        Checkbox::new(entry.links_override)
            .label("Set local links")
            .on_toggle(move |s: &mut State| {
                if let Some(e) = s.editor.as_mut().and_then(|e| e.services.get_mut(index)) {
                    e.links_override = !e.links_override;
                }
            })
            .build(k)
            .key("links-override"),
    );
    if !entry.links_override {
        for link in &entry.links {
            body = body.child(hint(
                format!("{} · {} (inherited)", link.label.text, link.url.text),
                k,
            ));
        }
        return body.child(hint(
            "Local links replace the repository's list. They open only when you choose them.",
            k,
        ));
    }
    for (i, link) in entry.links.iter().enumerate() {
        body = body.child(
            col()
                .gap(8.0)
                .child(field(
                    "Link label",
                    &link.label,
                    "e.g. Documentation",
                    &format!("link-label-{i}"),
                    index,
                    Field::LinkLabel(i),
                    k,
                ))
                .child(field(
                    "URL",
                    &link.url,
                    "https://…",
                    &format!("link-url-{i}"),
                    index,
                    Field::LinkUrl(i),
                    k,
                ))
                .child(
                    Button::new("Remove link")
                        .variant(ButtonVariant::Ghost)
                        .on_press(move |s: &mut State| {
                            if let Some(e) =
                                s.editor.as_mut().and_then(|e| e.services.get_mut(index))
                                && i < e.links.len()
                            {
                                e.links.remove(i);
                            }
                        })
                        .build(k),
                ),
        );
    }
    body.child(
        Button::new("Add link")
            .disabled(entry.links.len() >= 16)
            .on_press(move |s: &mut State| {
                if let Some(e) = s.editor.as_mut().and_then(|e| e.services.get_mut(index)) {
                    e.links.push(LinkEditor {
                        label: TextEdit::new(""),
                        url: TextEdit::new(""),
                        kind: None,
                    });
                }
            })
            .build(k)
            .key("add-link"),
    )
}
pub(super) fn availability(entry: &ServiceEditor, index: usize, k: &Kit) -> El {
    let mut choices = row().gap(4.0);
    for (name, value) in [
        ("Inherit", None),
        ("Enabled", Some(false)),
        ("Disabled", Some(true)),
    ] {
        choices = choices.child(
            Button::new(name)
                .variant(if entry.disabled == value {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::Ghost
                })
                .on_press(move |s: &mut State| {
                    if let Some(e) = s.editor.as_mut().and_then(|e| e.services.get_mut(index)) {
                        e.disabled = value;
                    }
                })
                .build(k),
        );
    }
    col()
        .gap(10.0)
        .child(hint("Profile entry", k))
        .child(choices)
        .child(hint(
            "Disabling an entry suppresses its metadata. It does not stop the process.",
            k,
        ))
}
