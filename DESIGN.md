---
name: Kossé
description: A compact native switchboard for local development services.
colors:
  accent: "#003DA5"
  accent-dark: "#91B3FF"
  canvas: "#F7F7F5"
  canvas-dark: "#18181A"
  ink: "#232326"
  ink-dark: "#EDEDEB"
  on-accent: "#FFFFFF"
  on-accent-dark: "#121C30"
  focus: "oklch(0.58 0.15 264)"
  focus-dark: "oklch(0.70 0.15 264)"
typography:
  wordmark:
    fontFamily: "Kibernetyk Mono"
    fontSize: "19px"
    fontWeight: 600
    lineHeight: "20px"
  body:
    fontFamily: "system-ui"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "20px"
  label:
    fontFamily: "system-ui"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: "20px"
  project-title:
    fontFamily: "system-ui"
    fontSize: "17px"
    fontWeight: 500
    lineHeight: "20px"
  inspector-title:
    fontFamily: "system-ui"
    fontSize: "19px"
    fontWeight: 500
    lineHeight: "20px"
  data:
    fontFamily: "ui-monospace"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: "20px"
rounded:
  control: "6px"
  row: "8px"
spacing:
  tight: "4px"
  related: "8px"
  field: "12px"
  controls: "16px"
  gutter: "20px"
  groups: "24px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.control}"
    typography: "{typography.label}"
    height: "24px"
    padding: "0 12px"
  button-primary-dark:
    backgroundColor: "{colors.accent-dark}"
    textColor: "{colors.on-accent-dark}"
    rounded: "{rounded.control}"
    typography: "{typography.label}"
    height: "24px"
    padding: "0 12px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    typography: "{typography.label}"
    height: "24px"
    padding: "0 12px"
  service-row:
    rounded: "{rounded.row}"
    padding: "10px 14px"
  inspector:
    width: "320px"
    padding: "20px"
---

# Design System: Kossé

## Overview

**Creative North Star: "Operate"**

A compact native switchboard of projects and worktrees. Kossé is read briefly beside an editor or terminal: recognise a service, inspect its identity, open it or explicitly confirm a stop. Neutral surfaces keep process facts prominent; Québec-inspired blue identifies primary actions and selected form choices.

This records the owner-approved native replacement of the retired browser dashboard. It was a code-led operational port, without a seeded concept roll or an approved composition. There is no retrospective seed, quality score or claim of visual approval. The authority is `crates/app/src/native/view.rs`, `customize.rs`, `customize/form.rs` and `customize/fields.rs`, with Rune and rune-kit pinned to `b63efb3d`. Earlier review screenshots predate the final density and blue-control changes. The surface composition is recorded separately in [.impeccable/surfaces/native.md](.impeccable/surfaces/native.md).

**Key Characteristics:**

- Compact project groups with stable process identity.
- Neutral light and dark appearance with a single action accent.
- Explicit observation text and quiet, current metrics.
- Structured customization with visible inheritance and a fixed save footer.

## Colors

### Primary

Québec blue is the primary action and selected-choice fill; its lighter companion serves dark appearance. The frontmatter records the native seed and explicit contrast overrides in authored sRGB. It intentionally does not copy unused web tokens.

### Neutral

Canvas and ink seed `Kit::from_palette`. Rune derives layers, muted text, borders, hover and pressed states in OKLab from that seed. Service rows use `layer_01`; inspected rows and the customization sidebar use `layer_02`. Those derived layers are not literal aliases of the surface swatches in [brand/tokens.json](brand/tokens.json). Default appearance follows macOS.

Focus currently uses rune-kit's neutral blue, recorded separately from the action accent. Danger controls and validation errors use the kit's semantic danger roles. Native endpoint observations are readable text; the retired browser marker palette is not a claim about this implementation.

**The Observation Rule.** An HTTP response is an observation, including an error status; blue selection and destructive red never imply service health.

## Typography

Native system sans carries interface text, medium-weight controls and project headings. System mono carries ports, paths, commands and compact CPU, memory and age values. Frontmatter sizes represent Rune logical points using portable `px` notation; they are not browser CSS instructions.

The identity uses the separated Relay K and Kibernetyk Mono wordmark defined in [brand/BRAND.md](brand/BRAND.md). The bundled wordmark assets and the native title use Semibold; the native title is 19 points. Preserve the original monospace advances. System fonts are for operational hierarchy, not an invented display identity.

## Layout

Use independent scrolling regions within a full-height window. A 50-point draggable title row reserves 90 points at its leading edge for window controls. Search and listener scope sit immediately below, with 20-point horizontal gutters. Project groups use a 24-point gap; group headings and rows use an 8-point gap. A 32-point footer keeps observation-note disclosure and keyboard hints outside the list.

Rows have a 72-point minimum height, 14-point horizontal and 10-point vertical padding. Wide rows align identity, endpoints, compact metrics and actions. Below 950 points, or with an inspector open, rows use two lines of content. The 320-point inspector sits alongside the list at wide widths and replaces the list at narrow widths. Returning from inspection preserves the local search and runtime identity.

Customization uses a 760-point dialog with a 160-point project/service sidebar and independently scrolling detail. Its body height is clamped between 280 and 600 points from the window height. Detail sections use 20-point padding and a 22-point gap. Save, Cancel and removal controls remain outside the scroll region in a 16-point padded footer.

## Elevation & Depth

Project groups are separated by space, service rows by tonal layers. Native rune-kit controls and dialogs retain the kit's subtle shadow and border treatment; this is not the retired web interface's shadowless system. There are no custom decorative gradients or ambient effects in the application layout. Control and overlay feedback use the pinned kit's motion; do not replace it with an unrelated animation vocabulary.

## Shapes

Rows use softly rounded 8-point corners. Controls inherit the kit's 6-point medium radius; dialog geometry belongs to the native kit. The Relay mark's open joint is intentional, and its menu-bar version is optically tuned separately from the large master. Use the generated assets rather than a typed K or a scaled approximation of a font glyph.

## Components

**Primary and selected buttons.** `Contrast` is explicitly overridden to Québec blue with the appropriate light/dark text. It identifies Save changes and the active project/service, protocol or availability choice. Native hover, focus, pressed and disabled states remain kit-owned.

**Quiet actions.** Ghost buttons carry Refresh, Inspect, Open, Customize, scope and disclosure actions. Standard buttons support Cancel and other secondary choices. Danger buttons identify Stop and confirmed removal. Text changes to Saving… or the observed stop result as a request progresses.

**Service rows.** The process identity is PID plus start time. Display names can be customized; package paths, endpoints and inspector facts retain the observed identity. Compact metrics show CPU and memory together and age below; unknown and warming-up remain explicit.

**Search and navigation.** Search filters project, branch, process, redacted command, package and port text. `/` focuses search only outside dialogs and an already focused search field; ordinary slash input inside customization remains text. Escape closes nonbusy overlays, clears selection and search. Do not document the retired browser's j/k row navigation as native behavior.

**Inspector.** A quiet facts pane with selectable, wrapping mono values for process, working directory, executable, parent, command and endpoints. Profile provenance and declared links appear beside the observed details. Link labels include the host and declared status.

**Customization.** Project and service navigation exposes labeled native fields. Empty names inherit; matching rules and links have explicit local-override controls. Protocol and availability include Inherit. Repository values remain read-only; saves target this Mac's central profile. Removing one entry changes the draft and requires Save changes; removing the whole customization has its own confirmation and persistence action. Detail and navigation are inert while saving. Neither stops a process.

**Stop confirmation.** Show process identity, project and every affected port. Force is a separate confirmation after a still-running result. Busy requests prevent dismissal; stale identity and failed outcomes are explicit text.

## Do's and Don'ts

### Do:

- Do retain observed identity when filtering, inspecting or customizing a display name.
- Do show unknown, warming-up, configured-only and ended states in words.
- Do keep Save and Cancel visible while customization details scroll.
- Do distinguish inherited repository values, local overrides and observations.

### Don't:

- Don't use the action accent as a service-health verdict.
- Don't infer that brand palette swatches equal every generated native kit color.
- Don't carry browser-only CSS, dialogs, keyboard shortcuts or font restrictions into native guidance.
- Don't substitute a font glyph for the Relay mark or alter the wordmark's monospace advances.
