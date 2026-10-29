//! The kit seeded from DESIGN.md and the few text and button styles every part of the view uses.
use crate::native::State;
use rune::{Color, El, TextStyle, text};
use rune_kit::{Button, ButtonVariant, Kit, PaletteSeed};

/// Data cells (ports, paths, metrics): system mono at 12 points.
pub const DATA_SIZE: f32 = 12.0;

pub fn kit(dark: bool) -> Kit {
    let rgb = |r: u8, g: u8, b: u8| Color {
        r: rune::color::srgb_decode(f32::from(r) / 255.0),
        g: rune::color::srgb_decode(f32::from(g) / 255.0),
        b: rune::color::srgb_decode(f32::from(b) / 255.0),
        a: 1.0,
    };
    let (bg, fg, accent, on_accent) = if dark {
        (
            rgb(24, 24, 26),
            rgb(237, 237, 235),
            rgb(145, 179, 255),
            rgb(18, 28, 48),
        )
    } else {
        (
            rgb(247, 247, 245),
            rgb(35, 35, 38),
            rgb(0, 61, 165),
            rgb(255, 255, 255),
        )
    };
    let mut kit = Kit::from_palette(PaletteSeed { bg, fg, accent });
    kit.c.bg_contrast = accent;
    kit.c.text_contrast = on_accent;
    kit.c.icon_contrast = on_accent;
    kit
}

pub fn label(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.ui).color(k.c.text_base)
}

pub fn muted(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.ui).color(k.c.text_muted)
}

pub fn mono(value: impl Into<String>, k: &Kit) -> El {
    text(value)
        .font(TextStyle {
            family: rune::Family::SystemMono,
            size: DATA_SIZE,
            ..k.ty.ui
        })
        .color(k.c.text_muted)
}

/// A column caption above data cells.
pub fn caption(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small_medium).color(k.c.text_faint)
}

/// A keyed button in one of the view's three roles.
pub fn button(
    value: &str,
    key: impl Into<String>,
    variant: ButtonVariant,
    k: &Kit,
    action: impl Fn(&mut State) + 'static,
) -> El {
    Button::new(value)
        .variant(variant)
        .on_press(action)
        .build(k)
        .key(key)
}

/// A secondary action with a visible outline: Refresh, Open, Inspect, Try HTTP.
pub const SECONDARY: ButtonVariant = ButtonVariant::Outline;
/// A quiet action that does not compete with the rows: Customize…, Dismiss, notes.
pub const QUIET: ButtonVariant = ButtonVariant::GhostMuted;

/// `42s`, `5m`, `3h 12m`, `2d 4h`.
pub fn age(seconds: u64) -> String {
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
