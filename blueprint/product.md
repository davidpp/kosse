# Product contract

## Primary jobs

1. Open the dashboard or tray menu and immediately see which projects are running.
2. Recognize the repository, worktree/branch, and package behind an unfamiliar port.
3. Open a valid local HTTP(S) service without remembering its port.
4. Inspect PID, working directory, bound addresses, process age, CPU and memory.
5. Stop one forgotten process safely, with explicit escalation if graceful stop fails.

No project registration or modified launch commands should be necessary. Optional machine-local project profiles improve attribution and links without touching repositories. UI customization writes centrally; repo-local metadata is an optional shareable alternative. See [project metadata](project-metadata.md). Observed runtime facts always remain distinguishable from declared intent.

## Dashboard surface

Served on loopback; shown in Kossé's own window (opened from the tray or on explicit launch) and reachable from any browser at the same URL.

One calm, dense dashboard grouped by repository/worktree, with package/service rows. Search matches project, package, command name and port. Default view filters known system noise; a visible “all listeners” option makes exclusions reversible.

Each row shows name, port(s), protocol/link confidence, process age, CPU, memory, observation state, and Open / Inspect / Stop actions. Different worktrees remain distinguishable even with identical repo names. One process owning multiple ports is one action target with multiple endpoints.

Inspect reveals exact identity, metadata availability, exposure classification, parent PID (if known), sanitized command, and why attribution/link inference was made. Missing permission or metadata displays unknown, not a guessed project.

Keyboard navigation, visible focus, accessible confirmation dialog, responsive narrow layout, light/dark system preference, reduced motion, and explicit empty/loading/stale/error states are required. No charts needed for v1; accurate current metrics suffice.

## Tray surface

A native macOS menu with project/service grouping, recognizable names/ports, Open, Inspect (opens the dashboard window on that service), Stop, Open Dashboard, and Quit. Stop must use the same confirmation and identity policy as the dashboard (opening the dashboard's confirmation is acceptable). No alternate unguarded destructive path.

Tray stays useful with browser closed. Initial discovery failure is visible. Quit stops Kossé only, never discovered services. Headless mode has no tray requirement.

## Observations, not unjustified health claims

- Listening: socket observed, regardless of HTTP behavior.
- Reachable HTTP: an HTTP response was received, including 401/403/404/500.
- Timeout/refusal/TLS failure: show the actual result separately from process/listener state.
- Do not label an API dead because GET / returns 404.
- HTTP probing is optional bounded enrichment. A TCP database gets no misleading Open link.
- HTTPS uses normal verification. Self-signed TLS failure is visible; do not globally disable TLS validation.
- Unknown listeners may offer a clearly labeled “try HTTP” action, never an assertion that they are websites.
- Disappeared listeners may remain briefly as ended rows (default 60 seconds, memory only); explain their last-seen timestamp. Restart creates a new process identity.
- Age alone is not proof that a service is stray; no automatic cleanup.

## Stop behavior

Confirm process name, PID, project, and all affected ports. Request graceful termination first. Force requires a new explicit confirmation after a still-running result. Never automatically escalate or kill a process group/parent. A known parent may explain likely respawn, not authorize killing it.

Revalidate identity immediately before signaling; stale identity is a conflict requiring refresh. Observe the port after signaling: distinguish exited, still running, endpoint now owned by another process, and observation failure. Do not equate probe failure with successful termination.

## Persistence and privacy

Store project profiles, aliases, hidden identities/rules, and UI preferences in a small versioned per-user config. “Customize this project” edits central metadata; normal use never writes into monitored repositories. Rules use stable project/path/service attributes, not recycled PIDs. Preserve unknown compatible fields where appropriate; reject unsupported newer schemas without overwriting. Atomic writes; corrupt config leaves recoverable original plus a clear warning.

No raw command line/environment persistence, external telemetry, update polling, outbound redirects during probing, or remote assets needed at runtime. Every displayed command passes centralized secret redaction; UI escapes process-derived strings.

## Terminal surface (next phase, after v1)

Planned, not part of v1. A first-party terminal UI for people who live in the terminal or don't want anything resident. `kosse tui` attaches to a verified running Kossé, or starts a private server for the session and stops it on exit. It offers the dashboard's feature set (grouped projects and services, search, all-listeners, inspect, Open in the default browser, try HTTP, the same Stop then Force confirmations and honest results, customize a project) with keyboard-first navigation, the brand palette mapped to the terminal, light/dark terminals and graceful narrow widths. It uses the same API and contracts; no alternate action policy.

## Use without a resident app

Nothing has to run in the background. `kosse open` starts the server, shows the dashboard window and exits when the window closes; `kosse list` is a one-shot scan. (The terminal client `kosse tui` follows in the next phase.) The resident tray app and login startup are opt-in.

## Explicit non-goals

Built-in DNS/reverse proxy/TLS certificate provisioning; start/restart orchestration; terminal/log capture; process-tree termination; container-internal attribution; metrics history database; plugin runtime/SDK in v1; network/storage cleanup; AI inference; custom tray popover; multiple-machine inventory.

Docker-owned host sockets can be labeled honestly. Mapping container services to host repos is not a v1 acceptance criterion.
