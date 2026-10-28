# Product

<!-- impeccable:product-schema 1 -->

<!--
Recorded during slice S01b from the repository (README.md, blueprint/product.md,
blueprint/architecture.md) and the owner's slice brief. The Impeccable interview was not run with
the owner in this session; facts marked (inferred) are derived from those documents and need the
owner's confirmation before S07 builds the dashboard on them.
-->

## Platform

web

## Users

Developers on macOS who start many local services from terminals, scripts and coding agents, often
across several git worktrees at once. They glance at Kossé between tasks to answer one question
quickly: what is running on this machine, and from which project? The dashboard is one of two
views; the other is a native macOS tray menu over the same core.

The primary dashboard surface is Kossé's own lightweight window (a WKWebView on the loopback
URL), opened from the tray. The same page also works in any browser. It is opened many times a
day for a few seconds each time, next to an editor and terminals (inferred), on both light and
dark system appearances.

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

- A resident local tool: one Rust process serves the dashboard on loopback; the UI is embedded in
  the binary and loads no remote assets.
- The user reaches the dashboard from the tray: its window opens, is read or acted on, and is
  closed again (inferred). It must stay cheap while closed and quick to open; a browser tab on the
  same URL is the secondary path.
- The user arrives with a port number, a project name or a vague "something is still running", and
  leaves by opening a link, reading process facts, or stopping one process.
- Observation is continuous; the page shows live and stale states rather than a one-off report.

## Capabilities and Constraints

- Required interactions: search across project, package, command and port; grouping by
  repository/worktree; Open / Inspect / Stop per service; an "all listeners" control that reverses
  default noise filtering; explicit stop confirmation, then a separate confirmation to force.
- Required states: empty, loading, stale, disconnected, error; ended rows remain briefly with their
  last-seen time.
- Process-derived strings (names, paths, commands) are untrusted text: escaped, and commands are
  redacted centrally before display.
- Performance budget: shipped JS + CSS at most 50 kB gzipped, no web fonts, system font stack.
  `bun run check` enforces it.
- Stack: React APIs on the TanStack Redact runtime, TanStack Query, Tailwind v4, Lucide icons,
  native `<dialog>`; hand-built components, no component framework.
- No charts in v1; current metrics only. No custom tray popover.

## Brand Commitments

- Name: Kossé. No logo or mark exists yet; do not invent one.
- Voice: plain, factual, calm. State what was observed and what to do next; no hype and no
  alarm beyond what the observation warrants (inferred from the README and product contract).

## Evidence on Hand

- Real data comes only from the running backend (`/api/health` today; the snapshot API arrives in
  S04). There are no screenshots, testimonials or usage figures; do not fabricate them.
- Owned fixture processes (`crates/fixtures`) provide real, labelled services for demonstrations
  and browser tests.

## Product Principles

1. Observed facts and declared intent stay visibly distinct; unknown is shown, never guessed.
2. Density serves recognition: every row carries enough identity to tell worktrees apart.
3. Destructive actions are slow on purpose; everything else is one keystroke or click away.
4. Calm by default: state changes are visible without demanding attention.
5. Nimble: the page must cost less than the services it watches.

## Accessibility & Inclusion

Keyboard navigation with visible focus, an accessible confirmation dialog, a responsive narrow
layout, light/dark from the system preference, and reduced motion are product requirements
(blueprint/product.md). State is never conveyed by colour alone.
