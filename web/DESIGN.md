---
name: Kossé
description: A calm, dense, keyboard-first local dashboard for development services.
colors:
  ink: "oklch(0.2574 0.0056 286.0023)"
  ink-muted: "oklch(0.4922 0.0142 285.8659)"
  canvas: "oklch(0.9756 0.0026 106.4483)"
  surface: "oklch(1 0 89.8756)"
  line: "oklch(0.6433 0.0148 285.9776)"
  line-strong: "oklch(0.5589 0.0153 285.8944)"
  accent: "oklch(0.4024 0.1765 261.2387)"
  focus: "oklch(0.4024 0.1765 261.2387)"
  listening: "oklch(0.4535 0.0178 285.67)"
  reachable: "oklch(0.5059 0.1771 261.6903)"
  stale: "oklch(0.4888 0.1022 77.0857)"
  failed: "oklch(0.5081 0.1625 17.8377)"
  unknown: "oklch(0.4922 0.0142 285.8659)"
  danger: "oklch(0.5081 0.1625 17.8377)"
  on-accent: "oklch(1 0 89.8756)"
  on-danger: "oklch(1 0 89.8756)"
  ink-dark: "oklch(0.9455 0.0027 106.4502)"
  ink-muted-dark: "oklch(0.7411 0.0142 286.0473)"
  canvas-dark: "oklch(0.2099 0.0039 286.0588)"
  surface-dark: "oklch(0.2574 0.0056 286.0023)"
  line-dark: "oklch(0.573 0.0167 285.8599)"
  line-strong-dark: "oklch(0.677 0.0175 285.9232)"
  accent-dark: "oklch(0.7715 0.1157 265.3437)"
  focus-dark: "oklch(0.7715 0.1157 265.3437)"
  listening-dark: "oklch(0.7983 0.014 286.0791)"
  reachable-dark: "oklch(0.7961 0.1024 263.9862)"
  stale-dark: "oklch(0.8095 0.1108 82.4964)"
  failed-dark: "oklch(0.7945 0.1219 9.3455)"
  unknown-dark: "oklch(0.7411 0.0142 286.0473)"
  danger-dark: "oklch(0.7945 0.1219 9.3455)"
  on-accent-dark: "oklch(0.2099 0.0039 286.0588)"
  on-danger-dark: "oklch(0.2099 0.0039 286.0588)"
  hairline: "color-mix(in oklab, {colors.line} 32%, {colors.surface})"
  row: "color-mix(in oklab, {colors.accent} 8%, {colors.surface})"
  failed-wash: "color-mix(in oklab, {colors.failed} 9%, {colors.surface})"
  stale-wash: "color-mix(in oklab, {colors.stale} 11%, {colors.surface})"
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
  dialog-title:
    fontFamily: "system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "1rem"
    fontWeight: 600
    letterSpacing: "-0.025em"
  data:
    fontFamily: "ui-monospace, SF Mono, Menlo, monospace"
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: "1.25rem"
  small:
    fontFamily: "system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: "1rem"
rounded:
  tag: "4px"
  control: "6px"
  panel: "8px"
spacing:
  gutter: "16px"
  card-gap: "12px"
  row-x: "16px"
  row-y: "10px"
components:
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "28px"
    padding: "0 10px"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "28px"
    padding: "0 6px"
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.control}"
    height: "28px"
  button-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.on-danger}"
    rounded: "{rounded.control}"
    height: "28px"
  project-card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.panel}"
  notice-failed:
    backgroundColor: "{colors.failed-wash}"
    textColor: "{colors.ink}"
    rounded: "{rounded.panel}"
    padding: "8px 12px"
---

# Design System: Kossé

Describes the dashboard as built in S07 (`web/src`). Colour roles come from
[brand/tokens.json](../brand/tokens.json) (W3C DTCG) and are mirrored in `web/src/styles.css`;
`src/tokens.test.ts` fails when the two differ. Identity, voice and the mark are in
[brand/BRAND.md](../brand/BRAND.md).

## Overview

A switchboard of projects: one card per worktree, one line per process with every port it owns
patched in. Operate mode throughout. It is opened from the tray for a few seconds at a time, so the
first viewport is the whole job: which projects run, on which ports, what HTTP saw, and the
actions. Density and consistency outrank expression; the brand lives in the Québec-blue focus ring,
primary actions and the header's Relay mark and Kibernetyk Mono wordmark, on neutral surfaces.

Light and dark follow the system appearance; there is no in-app switch. The only motion is state
feedback (150 ms colour transitions, a spinning refresh icon while a scan runs) and it collapses
under `prefers-reduced-motion`. Every colour value is inside sRGB.

## Colors

The strategy is Restrained: neutrals plus one accent, with observation markers as the only other
saturated colour on screen.

- `canvas` is the page; `surface` is raised: the chrome bar, cards, dialogs and controls.
- `line` is a meaningful boundary (card and dialog edges, notice borders) and meets 3:1. Inside a
  card, rows are separated by the derived `hairline`, which is decorative.
- `line-strong` bounds interactive controls (inputs, secondary buttons).
- `ink` carries text; `ink-muted` secondary text (≥ 4.5:1 on canvas and surface).
- `accent` (Québec blue, #003DA5 / #91B3FF) marks the mark, primary actions (Save), a service's open target and native control tint. `focus` is the
  2px keyboard ring with a 2px background gap; on service rows it is drawn inset (`row` wash plus
  the ring), because rows are focused with j/k.
- `danger` (Stop red) names the Stop action: red text on the row's Stop on hover and keyboard
  focus (muted at rest, see Buttons), and in Inspect; a solid red fill only on the final Stop and
  Force confirmations. Solid buttons use `on-accent` / `on-danger` text (surface in light, canvas
  in dark).
- Derived roles mix in OKLab (an OKLCH mix with a near-achromatic surface interpolates hue and
  shifted the row wash off-hue in WebKit): `hairline`, `row`, `failed-wash`, `stale-wash`.

### Named Rules

**The Marker Rule.** Observation colours (`listening`, `reachable`, `stale`, `failed`, `unknown`)
are for markers and icons only. Text that names a state stays `ink` or `ink-muted`.

**The Shape Rule.** Every state has its own marker shape and a text label, so none depends on
hue (`src/components/marker.tsx`). Markers sit at the start of each endpoint line (HTTP is per
port), and on the name for ended and configured rows:

| State                     | Meaning (product contract)                           | Marker                                    |
| ------------------------- | ---------------------------------------------------- | ----------------------------------------- |
| reachable                 | an HTTP response arrived (any status, including 404) | filled dot, `reachable`                   |
| listening                 | socket observed; not probed, or not HTTP             | dot in a ring, `listening`                |
| pending                   | HTTP not checked yet                                 | open ring, `listening`                    |
| failed                    | timeout, refusal, closed, TLS failure                | alert icon, `failed`                      |
| unknown / configured      | metadata missing; a profile entry nothing matches    | dashed ring, `unknown`, plus label        |
| ended                     | listener gone, kept 60 s                             | short bar, `unknown`, plus last-seen time |
| stale                     | the shown snapshot is older than expected            | half-filled ring, `stale`                 |
| disconnected / scan error | the page lost live updates, or scanning fails        | page-level notice, not a row marker       |

**The Closed Palette Rule.** Tailwind's default palette is removed (`--color-*: initial`) and the
theme is `static`, so only the roles above exist and all are defined in both schemes. A new colour
is a decision recorded here and in brand/tokens.json first.

**The Declared Rule.** Anything a profile declares (a name, a link) carries its provenance: the
tag "Kossé profile" (central) or "kosse.json" (repository). Declared links always show their host
and the tag "declared, local" or "declared, not verified"; they are never probed. A declared link
to the same URL as an endpoint's Open is not repeated on the row; Inspect lists every link.

## Typography

System sans for text, `ui-monospace` for data (ports, PIDs, paths, commands, metrics) at the
`data` size with tabular figures. Hierarchy comes from weight and colour at a fixed rem scale:
body 14px; card titles 14px semibold, -0.025em; dialog titles 16px semibold; `small` 12px for
metric labels, tags and hints. No web fonts ship (`bun run budget` fails on font files and
`@font-face`): the header wordmark is an inline SVG of Kibernetyk Mono SemiBold outlines
(`brand/kosse-wordmark.svg`) drawn in `ink`, beside the 16-point Relay mark in `accent`.

## Layout

- Chrome: a sticky 48px bar on `surface` with a `line` bottom edge: mark and name, search (with
  its `/` hint), the "All listeners" checkbox with the hidden count, live status, Refresh. Below
  640px it wraps to two rows (identity and status, then search and the checkbox).
- Content: a centred column up to 72rem with 16px gutters and 12px between cards.
- Card: header row (title, provenance, branch with the worktree tag and repository name when it
  differs, short path, Customize), then rows. Short paths keep the last two segments
  (`…/worktrees/storefront`); the full path is in Inspect.
- Row (≥ 768px): a three-column grid: identity (name, provenance, process and package), endpoints,
  then metrics (`up`, `cpu`, `mem`, values in `ink-muted`) and the Inspect / Stop actions. Every
  column starts on the same 20px first line: row buttons draw 24px but take 20px of layout. An
  endpoint line never wraps; its HTTP text truncates (full text in its tooltip) so Open and Try
  HTTP stay on the line. Narrow widths stack the three columns. The endpoint column is ordered by
  what a person does with it:
  1. the **open target** first: of the summary endpoints that answered HTTP (automatically or on
     request), a status below 400 with a page title, else a status below 400, else a title, else
     any response; the lowest port breaks ties. When nothing better answered it is a 4xx or 5xx,
     and its status stays on the line. Marker, the accent `Open` with the full `localhost:<port>`
     in ink, name, then the HTTP result with the page title. Order and emphasis only: every
     answering summary endpoint keeps an Open, as in the tray.
  2. other summary endpoints: marker, `:port`, name, exposure when not loopback-only, the HTTP
     result without the title (it repeats; the tooltip and Inspect have it), a quiet Open, Try
     HTTP. A failure here keeps the failed marker: it is a launch target.
  3. the muted `details` line (no marker, no Open, never the failed marker; endpoints separated by
     commas, since `·` separates a status from its title), then declared links.

  The tag `TCP` marks an endpoint whose probe got an answer that is not HTTP: observed, never a
  guess at "database". A known non-HTTP port or process name is a convention, not an observation:
  it keeps its "not probed: non-HTTP port/server" text and no tag. Unchecked and failed endpoints
  get none.

  There is no project-level lead: no observed fact makes one service a project's entry point.
  When two services serve the same app (an unconfigured Wrangler and its workerd both answer), the
  fix is a profile (`otherEndpoints: details`, `match.process`), which the prompt-first Customize
  window helps set up.

- Keyboard: `/` focuses search; `j`/`k` or the arrow keys move between rows (the arrows also from
  the search field); Enter on a row opens Inspect; Escape closes a dialog or clears search.
- Inspect is a right-docked sheet 34rem wide (full width below 640px); Stop and Customize are
  centred dialogs up to 32rem.

## Elevation & Depth

Flat. Layers separate by tone (`canvas` / `surface`) and 1px lines; no shadows. The modal backdrop
is `canvas` at 55%, with no blur.

## Shapes

4px tags, 6px controls, 8px panels and cards. Markers are 8px (the alert icon 14px).

## Components

- **Buttons.** Secondary: `surface` fill, `line-strong` border. Row action: borderless and
  `ink-muted` at rest, a border on hover, for Inspect, Stop (`danger` on hover and keyboard focus)
  and Customize. This is a deliberate trade-off: at rest Stop looks like Inspect, so it never
  outweighs the open target; the confirmation dialog, not the row's colour, is the safety cue.
  Light: a `hairline` border at rest (stronger on hover), in dialogs. Small: a quiet underlined
  text button inside a line of text ("Try HTTP"), `ink-muted` like a quiet Open, so an offer
  never outweighs what was observed. Primary: solid `accent` (Save). Danger: solid `danger`, only
  on the final confirmation. While a request runs a button is `aria-disabled` (still focusable)
  and says what it is doing ("Stopping…").
- **Open link.** The only `href` from data goes through `safeLink` (http/https, no credentials):
  the underlined label, then the host in mono, then an arrow. The open target's is `accent`
  (semibold label, `localhost:<port>` in ink). Another summary endpoint's is quiet: `Open ↗` in
  `ink-muted` with a `line` underline at rest, and `Open localhost:<port>` as its accessible name.
  Anything else renders as plain text.
- **Notices.** `failed-wash` + `failed` border (disconnected, refresh failed, scan error, no such
  process) or `stale-wash` + `stale` border (stale snapshot, failing scanner with last good data).
- **Dialogs.** Native modal `<dialog>`; focus starts on the safe button (Cancel, Done, Close) at
  every step; Escape closes. Stop shows process name, PID, project and every port, then the result;
  Force is a separate step offered only after `stillRunning`, with its expiry time. While a signal
  is being sent and observed, the dialog cannot be closed and offers no Cancel.
- **Loading.** Placeholder cards in the real card shape, not a spinner.

## Do's and Don'ts

### Do:

- Show unknown as unknown, with its dashed marker and words.
- Say what was observed and whether anything was sent ("Nothing was sent.").
- Keep native browser surfaces themed: focus ring, selection, caret, scrollbar, `accent-color`.

### Don't:

- Don't add a component framework, web font, inline `style` (the CSP is `style-src 'self'`) or
  colour outside the closed palette.
- Don't use the accent for status, a state colour for text, or one colour for two states.
- Don't add shadows, gradients, blur or coloured side borders.
- Don't render a URL from data without `safeLink`, or call a declared link verified.
