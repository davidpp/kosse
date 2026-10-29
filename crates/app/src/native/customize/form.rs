use super::{Editor, fields};
use crate::native::{State, client::Command};
use kosse_core::contract::SaveProfileRequest;
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, TextField};

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>, height: f32) {
    let Some(editor) = &s.editor else {
        return;
    };
    let mut body = col().h((height - 100.0).clamp(280.0, 600.0));
    if editor.remove_service.is_some() {
        body = body.child(col().padding(20.0).gap(14.0).grow(1.0).child(fields::hint("Remove this service's local entry?", k)).child(fields::hint("Any repository entry will apply again after you save. The running process is unaffected.", k)));
    } else if editor.remove {
        body = body.child(
            col()
                .padding(20.0)
                .gap(14.0)
                .grow(1.0)
                .child(fields::hint(
                    "Remove all local names, matching rules and links for this project?",
                    k,
                ))
                .child(fields::hint(
                    "Repository settings will apply again. Running processes are unaffected.",
                    k,
                )),
        );
    } else {
        let detail = editor
            .selected
            .and_then(|i| editor.services.get(i).map(|entry| (i, entry)))
            .map_or_else(
                || project(editor, k),
                |(i, entry)| {
                    col()
                        .gap(22.0)
                        .child(fields::identity(entry, i, k))
                        .child(fields::matching(entry, i, k))
                        .child(fields::protocol(entry, i, k))
                        .child(fields::links(entry, i, k))
                        .child(fields::availability(entry, i, k))
                },
            )
            .padding(20.0)
            .grow(1.0)
            .min_w(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            });
        let detail = if editor.busy { detail.inert() } else { detail };
        body = body.child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .child(if editor.busy {
                    sidebar(editor, k).inert()
                } else {
                    sidebar(editor, k)
                })
                .child(detail),
        );
    }
    if let Some(error) = &editor.error {
        body = body.child(
            text(error)
                .wrap()
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .px(16.0)
                .py(8.0),
        );
    }
    body = body
        .child(el().h(1.0).fill(k.c.border_weak))
        .child(controls(editor, k).padding(16.0));
    Dialog::new(
        "customize",
        if editor.remove {
            "Remove local customization?"
        } else {
            "Customize project"
        },
    )
    .open(true)
    .fit()
    .width(760.0)
    .click_outside(false)
    .on_close(|s: &mut State| {
        if s.editor.as_ref().is_some_and(|e| !e.busy) {
            s.editor = None;
        }
    })
    .child(body)
    .build(k, layers);
}
fn project(editor: &Editor, k: &Kit) -> El {
    col().gap(16.0).child(text("Project").font(k.ty.ui_medium).color(k.c.text_base))
        .child(fields::hint(&editor.root, k).wrap_anywhere().selectable(true))
        .child(TextField::new("Display name", &editor.name).placeholder(&editor.inherited_name).disabled(editor.busy)
            .on_edit(|s: &mut State| s.editor.as_mut().map_or(&mut s.orphan_edit, |e| &mut e.name)).build(k).key("project-name"))
        .child(fields::hint("Leave blank to inherit the repository name, or use the directory name.", k))
        .child(fields::hint("Saved on this Mac. Your repository's kosse.json stays unchanged.", k))
        .child(fields::hint("Add a service to give a process a name, match it by directory or port, and attach useful links.", k))
}
fn sidebar(editor: &Editor, k: &Kit) -> El {
    let mut list = col()
        .w(160.0)
        .gap(4.0)
        .padding(12.0)
        .fill(k.c.layer_02)
        .child(
            Button::new("Project")
                .variant(if editor.selected.is_none() {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::Ghost
                })
                .on_press(|s: &mut State| {
                    if let Some(e) = &mut s.editor {
                        e.selected = None;
                    }
                })
                .build(k)
                .key("edit-project"),
        )
        .child(fields::hint("Services", k).mt(12.0));
    for (i, entry) in editor.services.iter().enumerate() {
        list = list.child(
            Button::new(if entry.id.text.is_empty() {
                "New service"
            } else {
                &entry.id.text
            })
            .variant(if editor.selected == Some(i) {
                ButtonVariant::Contrast
            } else {
                ButtonVariant::Ghost
            })
            .on_press(move |s: &mut State| {
                if let Some(e) = &mut s.editor {
                    e.selected = Some(i);
                }
            })
            .build(k)
            .key(format!("edit-service-{i}")),
        );
    }
    list.child(
        Button::new("+ Add service")
            .disabled(editor.busy || editor.services.len() >= 64)
            .variant(ButtonVariant::Ghost)
            .on_press(|s: &mut State| {
                if let Some(e) = &mut s.editor {
                    e.add();
                }
            })
            .build(k)
            .key("add-service"),
    )
    .scroll(ScrollSpec {
        stick_bottom: false,
    })
}
fn controls(editor: &Editor, k: &Kit) -> El {
    if let Some(index) = editor.remove_service {
        return remove_service_controls(index, k);
    }
    if editor.remove {
        return row()
            .gap(8.0)
            .child(
                Button::new("Keep customization")
                    .disabled(editor.busy)
                    .on_press(|s: &mut State| {
                        if let Some(e) = &mut s.editor {
                            e.remove = false;
                        }
                    })
                    .build(k),
            )
            .child(el().grow(1.0))
            .child(
                Button::new("Remove customization")
                    .variant(ButtonVariant::Danger)
                    .disabled(editor.busy)
                    .on_press(|s: &mut State| {
                        if let Some(e) = &mut s.editor {
                            e.busy = true;
                            let request = SaveProfileRequest {
                                root: e.root.clone(),
                                profile: None,
                            };
                            s.send(Command::Save(request));
                        }
                    })
                    .build(k)
                    .key("confirm-remove-profile"),
            );
    }
    row()
        .gap(8.0)
        .child(
            Button::new(if editor.selected.is_some() {
                "Remove entry…"
            } else {
                "Remove customization…"
            })
            .variant(ButtonVariant::Ghost)
            .disabled(editor.busy)
            .on_press(|s: &mut State| {
                if let Some(e) = &mut s.editor {
                    if let Some(index) = e.selected {
                        e.remove_service = Some(index);
                    } else {
                        e.remove = true;
                    }
                }
            })
            .build(k)
            .key("remove-profile"),
        )
        .child(el().grow(1.0))
        .child(
            Button::new("Cancel")
                .disabled(editor.busy)
                .on_press(|s: &mut State| s.editor = None)
                .build(k)
                .key("cancel-profile"),
        )
        .child(
            Button::new(if editor.busy {
                "Saving…"
            } else {
                "Save changes"
            })
            .variant(ButtonVariant::Contrast)
            .disabled(editor.busy)
            .on_press(|s: &mut State| {
                if let Some(e) = &mut s.editor {
                    match e.request() {
                        Ok(request) => {
                            e.busy = true;
                            e.error = None;
                            s.send(Command::Save(request));
                        }
                        Err(error) => e.error = Some(error),
                    }
                }
            })
            .build(k)
            .key("save-profile"),
        )
}

fn remove_service_controls(index: usize, k: &Kit) -> El {
    row()
        .gap(8.0)
        .child(
            Button::new("Keep entry")
                .on_press(|s: &mut State| {
                    if let Some(e) = &mut s.editor {
                        e.remove_service = None;
                    }
                })
                .build(k),
        )
        .child(el().grow(1.0))
        .child(
            Button::new("Remove entry")
                .variant(ButtonVariant::Danger)
                .on_press(move |s: &mut State| {
                    if let Some(e) = &mut s.editor {
                        if index < e.services.len() {
                            e.services.remove(index);
                        }
                        e.selected = None;
                        e.remove_service = None;
                    }
                })
                .build(k)
                .key("confirm-remove-service"),
        )
}
