//! The Stop confirmation: identity, project and every affected port, each its own element so no
//! process-derived text can imitate another line. Force is a second confirmation.
use super::{
    State, display_name, port_list,
    theme::{caption, label, mono},
};
use crate::{native::Confirmation, native::client::Command, text::printable};
use kosse_core::action::ActionResult;
use rune::{El, col, row};
use rune_kit::{Button, ButtonVariant, Dialog, Kit};

const DIALOG_W: f32 = 510.0;

pub fn view(dialog: &Confirmation, s: &State, k: &Kit, layers: &mut Vec<El>) {
    let heading = if dialog.busy {
        "Waiting for the process…"
    } else if dialog.force {
        "Force this process to stop?"
    } else if dialog.result.is_some() {
        "Stop result"
    } else {
        "Stop this process?"
    };
    let fact = |title: &str, value: String| {
        col()
            .gap(2.0)
            .child(caption(title, k))
            .child(mono(value, k).wrap_anywhere())
    };
    let mut body = col()
        .padding(20.0)
        .gap(12.0)
        .child(fact(
            "Process",
            format!(
                "{} · PID {}",
                display_name(&dialog.service, s),
                dialog.request.identity.pid
            ),
        ))
        .child(fact(
            "Project",
            dialog
                .service
                .project
                .as_deref()
                .map_or_else(|| "No attributed project".into(), printable),
        ))
        .child(
            label(
                format!(
                    "All affected ports: {}",
                    port_list(dialog.request.ports.iter().copied())
                ),
                k,
            )
            .key("affected-ports")
            .wrap(),
        );
    if let Some(result) = &dialog.result {
        body = body.child(label(result_text(result), k).key("stop-result").wrap());
    }
    if let Some(error) = &dialog.error {
        body = body.child(label(printable(error), k).color(k.c.danger.fg).wrap());
    }
    body = body.child(controls(dialog, k).pt(4.0));
    Dialog::new("stop", heading)
        .open(true)
        .fit()
        .width(DIALOG_W)
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
        ActionResult::Exited { shared, .. } if shared.is_empty() => {
            "Process exited. Its ports are free.".into()
        }
        ActionResult::Exited { .. } => {
            "Process exited. Other processes still share these ports.".into()
        }
        ActionResult::StillRunning { .. } => {
            "Still running after graceful termination. Force requires another confirmation.".into()
        }
        ActionResult::Respawned { .. } => "The process exited, but its supervisor started a \
                                           replacement. The parent was not stopped."
            .into(),
        ActionResult::EndpointTaken { .. } => {
            "The process exited. Another process now owns an affected port.".into()
        }
        ActionResult::Stale { .. } => {
            "The process or ports changed. Nothing was sent. Refresh and confirm again.".into()
        }
        ActionResult::ForceNotOffered => {
            "The force offer expired or is unavailable. Nothing was sent.".into()
        }
        ActionResult::Refused { .. } => "Kossé cannot stop this process. Nothing was sent.".into(),
        ActionResult::PermissionDenied { .. } => "The operating system refused permission.".into(),
        ActionResult::ObservationFailed { reason, .. } => format!(
            "Signal sent; the outcome could not be observed: {}",
            printable(reason)
        ),
        ActionResult::SignalFailed { reason, .. } => {
            format!("No signal was sent: {}", printable(reason))
        }
        ActionResult::Internal { reason } => {
            format!("Outcome unknown. Refresh: {}", printable(reason))
        }
    }
}

fn controls(dialog: &Confirmation, k: &Kit) -> El {
    let mut controls = row().gap(8.0).justify_end().child(
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
