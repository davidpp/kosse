# Product

<!-- impeccable:product-schema 1 -->

<!--
Recorded in S01b from the repository and the owner's slice brief; confirmed by the owner on
2026-10-28 for S07 (usage, layout and voice below).
-->

## Platform

macOS native (Rune)

## Users

Developers on macOS who start many local services from terminals, scripts and coding agents, often
across several git worktrees at once. They glance at Kossé between tasks to answer one question
quickly: what is running on this machine, and from which project? The dashboard is one of two
views; the other is a native macOS tray menu over the same core.

The primary surface is the native Rune window opened from the tray or `kosse open`. It is used briefly beside editors and terminals: find a port, inspect or act, then close. The browser UI is retired by owner direction (2026-10-29). Light and dark system appearance, keyboard access and narrow windows matter.

## Product Purpose

Kossé discovers listening services, attributes each to its repository, worktree and package,
links to it when it speaks HTTP, shows its process facts and current CPU and memory, and stops a
forgotten process only after confirming its exact identity. Success is that the user recognises an
unfamiliar port in seconds and never stops the wrong process.

## Positioning

It observes rather than orchestrates: no project registration, launch wrappers, proxies or port
allocation. It reports observations, never inferred health: listening, reachable over HTTP (any
status, including 404), or a specific failure, with unknown shown as unknown.

## Operating Context

- A resident local tool: one Rust owner scans and serves a guarded loopback API and SSE stream.
  A native Rune window child and the tray read that shared state; the CLI provides terminal access.
  The native UI is compiled into the executable and loads no remote assets.
- The user reaches the dashboard from the tray: its window opens, is read or acted on, and is
  closed again. It must stay cheap while closed and quick to open; `kosse list` is the terminal path.
- The user arrives with a port number, a project name or a vague "something is still running", and
  leaves by opening a link, reading process facts, or stopping one process.
- Observation is continuous; the native window shows live and stale states rather than a one-off report.

## Capabilities and Constraints

- Layout (owner, 2026-10-28): one card per project/worktree with its service rows (state shape,
  name, ports, HTTP result, metrics); Inspect opens as a side sheet (a full pane on narrow widths);
  search filters cards and rows.
- Required interactions: search across project, package, command and port; grouping by
  repository/worktree; Open / Inspect / Stop per service; an "all listeners" control that reverses
  default noise filtering; explicit stop confirmation, then a separate confirmation to force.
- Required states: empty, loading, stale, disconnected, error; ended rows remain briefly with their
  last-seen time.
- Process-derived strings (names, paths, commands) are untrusted text: escaped, and commands are
  redacted centrally before display.
- Stack: Rune and rune-kit, pinned to upstream Git revision `b63efb3d`. Native controls, one guarded API client and the existing Rust scanner owner.
- Record native binary size and owner/window memory separately; no web assets or remote fonts.
- No charts in v1; current metrics only. No custom tray popover.

## Brand Commitments

- Identity: [brand/BRAND.md](brand/BRAND.md) (mark, Relay mark and Québec-inspired blue, roles in
  [brand/tokens.json](brand/tokens.json)). Use the mark's geometry; never substitute a font K.
- Voice (owner, 2026-10-28): strictly plain, calm and factual, written like a developer tool for
  developers looking after their own dev services. State what was observed and what to do next; no
  hype, no alarm beyond the observation, no Québécois flavour or French in the interface. « Kossé »
  is the name's etymology, not positioning or a tagline.

## Evidence on Hand

- Real data comes only from the running backend (`/api/snapshot` and `/api/events`). There are no
  testimonials or usage figures; do not fabricate them. Screenshots in `docs/screenshots/` show
  fixture processes only (captured through `bun run app`).
- Owned fixture processes (`crates/fixtures`) provide real, labelled services for demonstrations
  and native-window tests.

## Product Principles

1. Observed facts and declared intent stay visibly distinct; unknown is shown, never guessed.
2. Density serves recognition: every row carries enough identity to tell worktrees apart.
3. Destructive actions are slow on purpose; everything else is one keystroke or click away.
4. Calm by default: state changes are visible without demanding attention.
5. Nimble: the interface must cost less than the services it watches.

## Accessibility & Inclusion

Keyboard navigation with visible focus, an accessible confirmation dialog, a responsive narrow
layout, light/dark from the system preference, and reduced motion are product requirements
(blueprint/product.md). State is never conveyed by colour alone.
