---
version: 1
slug: "src-app-tsx"
primary_target: "src/app.tsx"
related_targets: []
---

# Dashboard (/)

Mode: Operate. Glance surface opened from the tray many times a day for seconds: find the port or
project, act, close. Keyboard first, instant scan, fast first paint, cheap while closed.

## Direction contract

THESIS: A switchboard of projects: each card is one worktree, each line one process with every
port it owns patched in. Refuses the flat process table that hides which checkout a port belongs to.

OWN-WORLD: Brand "Le raccord": green-tinted neutral canvas, white raised cards with 1px Joint
boundaries, Fleuve only for focus/selection/primary, observation markers by shape (filled dot,
ring, dashed ring, alert glyph), Arrêt red as text on rows and solid only on the final confirm.
System sans, mono tabular data.

STORY: The developer sees which projects run, recognises the worktree/branch, opens or inspects a
port, and stops one process after confirming every port it serves.

FIRST VIEWPORT: 48px chrome: mark + Kossé, search (/), All listeners toggle with hidden count,
live status + refresh at right. Below, project cards in one column (max 72rem): header with name,
branch, path tail, Customize; rows: marker, name/provenance, endpoints with HTTP result and Open,
age/CPU/memory, Inspect/Stop.

FORM: Owner-pinned layout (project cards + side sheet); no concept roll (pinned brief wins).

FINISH: met in S07 by two review lenses (quality, docs/design) and their fixes, DESIGN.md
rewritten from the built UI, and screenshots generated from fixture data only (docs/evidence/S07.md).
