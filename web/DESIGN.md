---
name: Kossé
description: A calm, dense, keyboard-first local dashboard for development services.
colors:
  canvas: "oklch(0.985 0.002 250)"
  surface: "oklch(0.962 0.004 250)"
  line: "oklch(0.89 0.006 250)"
  line-strong: "oklch(0.62 0.012 250)"
  ink: "oklch(0.24 0.012 250)"
  ink-muted: "oklch(0.47 0.012 250)"
  accent: "oklch(0.5 0.13 262)"
  accent-ink: "oklch(0.985 0.002 250)"
  reachable: "oklch(0.52 0.12 155)"
  stale: "oklch(0.54 0.11 70)"
  failed: "oklch(0.53 0.17 27)"
  failed-wash: "oklch(0.962 0.018 27)"
  canvas-dark: "oklch(0.18 0.006 250)"
  surface-dark: "oklch(0.215 0.008 250)"
  line-dark: "oklch(0.31 0.01 250)"
  line-strong-dark: "oklch(0.53 0.012 250)"
  ink-dark: "oklch(0.93 0.006 250)"
  ink-muted-dark: "oklch(0.72 0.01 250)"
  accent-dark: "oklch(0.74 0.11 262)"
  accent-ink-dark: "oklch(0.18 0.006 250)"
  reachable-dark: "oklch(0.74 0.13 155)"
  stale-dark: "oklch(0.79 0.12 75)"
  failed-dark: "oklch(0.72 0.15 27)"
  failed-wash-dark: "oklch(0.25 0.04 27)"
typography:
  body:
    fontFamily: "system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.45
  title:
    fontFamily: "system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 600
    letterSpacing: "-0.025em"
  data:
    fontFamily: "ui-monospace, SF Mono, Menlo, monospace"
    fontSize: "0.8125rem"
    fontWeight: 400
rounded:
  control: "6px"
  panel: "8px"
spacing:
  gutter: "16px"
  section: "20px"
components:
  button-secondary:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "28px"
    padding: "0 10px"
  button-secondary-hover:
    backgroundColor: "{colors.surface}"
  alert-failed:
    backgroundColor: "{colors.failed-wash}"
    textColor: "{colors.ink}"
    rounded: "{rounded.panel}"
    padding: "10px 12px"
---

<!-- PROVISIONAL: recorded from the S01b foundation (web/src/styles.css). The full visual direction
round for the dashboard is S07's, with the owner; re-run `$impeccable document` after S07 so this
file describes the built dashboard. Tokens here are normative for anything built before then. -->

# Design System: Kossé

## Overview

An instrument panel, not a report: quiet neutral chrome with a faint cool tint, so observation
markers are the only saturated colour on screen. Operate mode throughout. The page should disappear
into the task of recognising a port and acting on it; density and consistency outrank expression.

Light and dark follow the system appearance with no in-app switch. Motion is limited to state
feedback (150 ms colour transitions, a spinner while a request is in flight) and collapses under
`prefers-reduced-motion`. Every colour value is inside sRGB.

## Colors

### Primary

Accent (`accent`) marks focus rings, text selection, the caret, native control tint and, later,
primary actions; `accent-ink` is text on an accent fill. The accent never colours an observation
state and never decorates.

### Neutral

`canvas` is the page, `surface` the second layer for chrome bars and panels, `line` decorative
hairlines (dividers, panel edges). `line-strong` is the boundary of interactive controls and meets
3:1 against `canvas`, `surface` and `failed-wash` in both schemes. `ink` carries text; `ink-muted`
carries secondary text and meets 4.5:1 on `canvas` and `surface`.

### Named Rules

**The Marker Rule.** Observation colours (`reachable`, `stale`, `failed`) are for markers and icons
only, and meet 3:1 against the grounds they sit on. Text that names a state stays `ink` or
`ink-muted`. Light `stale` also meets 4.5:1, so it does not fail if it ever lands on text.

**The Shape Rule.** Every state has a distinct marker shape and a text label, so no state depends on
hue:

| State        | Meaning (product contract)                           | Marker                                        |
| ------------ | ---------------------------------------------------- | --------------------------------------------- |
| listening    | socket observed; no HTTP claim                       | filled dot, `ink-muted`                       |
| reachable    | an HTTP response arrived (any status, including 404) | filled dot, `reachable`                       |
| failed       | a specific failure (timeout, refused, TLS)           | alert icon, `failed`                          |
| stale        | the shown snapshot is older than expected            | hollow ring, `stale`                          |
| unknown      | metadata or permission missing                       | dashed ring, `ink-muted`                      |
| ended        | listener gone, kept briefly                          | hollow ring, `ink-muted`, plus last-seen time |
| disconnected | the page lost its live updates                       | page-level notice, not a per-row marker       |

Listening and reachable are separate states and never share a colour. The connected server on the
S01b health page is reachable (it answered `/api/health`).

**The Closed Palette Rule.** Tailwind's default palette is removed (`--color-*: initial`) and the
theme is `static`, so only the roles above exist and all of them are defined in both schemes. A new
colour is a design decision recorded here first.

## Typography

One system sans for every role; hierarchy comes from weight and colour, not size jumps, at a fixed
rem scale. Body is 14px. Titles are semibold with -0.025em tracking. The `data` size (13px,
`text-data`) with the monospace face is for data only: ports, PIDs, versions, paths, with tabular
numerals so columns align. No web fonts ship (`bun run budget` fails on font files and
`@font-face`).

## Layout

A 44px chrome bar on `surface` with a bottom hairline, then content in a centred column up to
64rem with 16px gutters. Narrow widths (below 640px) drop secondary chrome text before anything
wraps; content panels wrap their controls under the message instead of clipping.

## Elevation & Depth

Flat. Layers separate by tone (`canvas` / `surface`) and 1px hairlines; there are no shadows.

## Shapes

Small, even radii: 6px for controls, 8px for panels. State markers are 8px, per the Shape Rule.

## Components

### Buttons

Secondary button: `canvas` fill, `line-strong` border, medium-weight label with an optional 14px
Lucide icon. Hover moves to `surface`; press nudges down 1px. While its request runs it is
`aria-disabled` (still focusable, clicks and keys ignored), shows the busy label ("Retrying…") with a
spinning icon, and lowers opacity.

### Alerts

A failure panel is `failed-wash` with a `failed` hairline at 35% and a `failed` icon; the message
names the problem in `ink` and the panel carries its recovery action inline.

## Do's and Don'ts

### Do:

- Show unknown as unknown, with its dashed marker and words.
- Use the mono face at the `data` size for values a user might copy or compare.
- Keep native browser surfaces themed: focus ring, selection, caret, scrollbar, `accent-color`.

### Don't:

- Don't introduce a component framework, web font or colour outside the closed palette.
- Don't use the accent for status, a state colour for text or decoration, or one colour for two
  states.
- Don't add shadows, gradients, or coloured side borders on panels.

<!-- Open for S07 (no token yet; decide with the owner during the direction round):
- selected-row wash and the keyboard-cursor row treatment;
- keyboard hint (kbd) style;
- row height and in-row spacing tokens for the dense service list;
- destructive-action role for Stop / Force (distinct from the `failed` observation colour);
- the disconnected notice and the ended-row treatment, validated with real fixtures. -->
