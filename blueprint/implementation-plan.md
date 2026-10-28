# Implementation plan

This is the dependency/specification index, not a second status tracker. Live state and evidence live in the maintainer's tracker; durable evidence summaries are committed under `docs/evidence/<slice>.md`. A slice is runnable only when its dependencies are done.

| Plan | Work package | Depends on |
|---|---|---|
| S01 | Scaffold Rust/React workspace and executable quality gates | — |
| S01b | Slim the web stack onto the TanStack Redact runtime within the performance budget | S01 |
| S02 | Validate macOS discovery and build owned-process fixture harness | S01 |
| S03 | Implement project attribution, shared snapshots and process metrics | S02 |
| S04 | Add secure local API, SSE and bounded HTTP enrichment | S03 |
| S05 | Implement identity-safe stop and explicit force escalation | S04 |
| S06 | Add central project profiles and optional repo metadata | S03 |
| S07 | Build the slim dashboard and verify real service workflows | S01b, S04, S05, S06 |
| S08 | Add native macOS tray lifecycle, on-demand `kosse open` and single-instance behavior | S07 |
| S09 | Implement local packaging, install/uninstall and opt-in login startup | S08 |
| S10 | Run installed-entrypoint acceptance audit and complete operator docs | S09 |

## Dependency graph

```text
S01 → S02 → S03 → S04 → S05 ─┐
 │             └→ S06 ──────┼→ S07 → S08 → S09 → S10
 └→ S01b ───────────────────┘
```

S01b (web only, after S01) and S06 (after S03) may proceed independently of S04/S05; use separate worktrees only when explicitly coordinated. Default loop order is table order. Required v1 includes passive project metadata; external proxy/port allocator integration is limited to declared friendly links, not management.

## S01

### Scope

Create Cargo core/app workspace and web via supported current Vite/React + TanStack Router generators; Bun lockfile, Tailwind v4/shadcn CLI, TanStack Query, strict TS. Set tested toolchain, Oxlint/Oxfmt, Lefthook, Rust fmt/clippy, unit/UI test infrastructure and CI config. Establish a real root command runner, minimal embedded frontend build, version/health entrypoint, typed contract generation with freshness check, and development process cleanup. Do not add placeholder release scripts.

### Acceptance

From a clean dependency install, dev serves a minimal page through the intended dev proxy; production binary serves embedded page; actual check/build/test/fmt:check commands pass. Generated type staleness fails a test/check. Hook is non-mutating and CI uses same checks. Document versions, commands, and missing platform prerequisites; the public remote exists, so CI runs on push. Read current docs and inspect existing files first.

## S01b

Added 2026-10-28 by owner direction: Kossé is a resident tool and must stay nimble; the S01 web scaffold (React, TanStack Router/Query, shadcn/Base UI, Geist) shipped ~103 kB gzipped JS for one status line.

### Scope

Move the web app onto the slim stack in architecture.md: React APIs on the TanStack Redact runtime (Vite plugin, also in Vitest so tests run what ships), TanStack Query, Tailwind v4 (build-time), per-icon Lucide, system fonts; remove shadcn/Base UI, Geist and other runtime weight. Measure the bundle with and without TanStack Router and keep it only within budget, recording the numbers. Keep the S01 behaviour (health page, dev proxy, embedded build, contract freshness) and gates. Add an enforced bundle-size budget to `check`. Remove dependencies that no longer have users. Establish the Impeccable design context for the dashboard (no full dashboard yet; that is S07).

### Acceptance

`bun run check` passes from a clean install; the budget check fails when exceeded (demonstrated) and the shipped bundle is within 50 kB gzipped; `bun run dev` and the release binary serve the page with the health result; web tests run with Vitest + Testing Library under the Redact runtime; docs and generators updated; evidence records before/after sizes.

## S02

### Scope

Implement TCP listener→PID discovery and process inspection; evaluate listeners vs lsof fixture ground truth, fallback netstat2 only with evidence. Add owned fixture process manifest/identity cleanup, IPv4/IPv6, multi-port, non-HTTP, process-exit races, unavailable metadata, timed child teardown. Add kosse list --json with actual local observations; no real user service signaling.

### Acceptance

CLI output matches known fixture PIDs/ports and lsof; duplicates normalized without hiding bind addresses. Permission and disappearing-process cases yield partial warnings, not panic/false empty success. Fixture cleanup only touches recorded owned identities. Record measured scan cost, chosen backend and crate licenses.

## S03

### Scope

Build one scheduler/core model per blueprint/architecture.md. PID+start identity, worktree/repo grouping, nearest package labels, branch/detached HEAD, endpoint dedupe, bounded cache, retained CPU sampling, memory and age. Coalesce refreshes and retain last-good snapshot with staleness. Core never depends on UI.

### Acceptance

Actual list JSON plus deterministic tests distinguish same-name repos, nested packages and two worktrees; .git file and Unicode paths supported. CPU warmup is unknown then sampled on controlled workload. 30-listener scan duration measured against 3-second cadence; scanner errors/overruns visible. No repeated per-port process introspection.

## S04

### Scope

Axum read API, SSE and revisioned snapshots over single scheduler. Exact loopback bind/Host guard, same-origin unsafe request policy, JSON/content limits and per-instance token. Central command redaction. Bounded candidate HTTP probing, 404 reachability, TLS errors, IPv6, redirect restriction and stream-body cap. Dev proxy has explicit dev-only authority, no production relaxation.

### Acceptance

Real HTTP integration suite rejects malicious Host/Origin/missing token/cross-site payloads and accepts intended same-origin requests. Raw secrets never appear in API/logs. 404 is reachable, hanging/huge/redirect endpoints bounded, non-HTTP sockets not blindly probed. SSE reconnect/revision behavior and last-good staleness verified. Occupied default port fails clearly.

## S05

### Scope

Add core action policy and secured API for own-user eligible PID+start identities. Self/invalid/unknown identity refusal; immediate revalidation, TERM then separate confirmed KILL, bounded post-signal observations and respawn reporting. Never execute cmdlines or signal groups/parents. Explain remaining macOS check/signal race.

### Acceptance

Real API tests terminate only owned fixtures; stale identity/mismatch/self/foreign owner refused. TERM-ignoring fixture remains until explicit second action. Supervisor respawn is distinguished from released port; observation error cannot report success. Confirmations include all ports sharing target identity. No real user service stopped.

## S06

### Scope

Implement blueprint/project-metadata.md: central project profiles keyed by canonical worktree path, schema emission/validation, optional bounded repo-file read/reload, service labels/matches/probe paths/friendly links and honest configured-not-observed state. Central-over-repo-over-inference precedence with provenance. Versioned atomic preferences; corruption/newer-schema safety. UI customization writes centrally with zero repository footprint. No scripts, launch, proxy, CA, port reservation or plugin runtime.

### Acceptance

Fixture tests cover traversal/symlinks, ambiguity, multiple processes in cwd, port-only misattribution prevention, malicious links, corrupt/newer schema, failed reload retaining last-good warning, config deletion, and worktrees. Emitted schema validates shipped example. CLI shows provenance and honest unobserved state. Verify central overrides, worktree isolation, explicit moved-path reassociation and that customization leaves monitored repo status and files unchanged. Metadata causes no execution, system/network configuration changes or remote fetching.

## S07

### Scope

Use Impeccable (design context, then craft, critique and audit) and the slim Redact stack in architecture.md, within its performance budget, to implement dense grouped dashboard, search, endpoint links, metrics, inspect route, all-listeners/noise controls, aliases, metadata provenance, explicit stop/force dialogs, responsive/theme/accessibility/error/stale/empty states. TanStack Query owns server state with SSE integration (revision-ordered). Wire “Customize this project” and persisted central profiles/preferences through guarded API; consume generated contracts. Display effective metadata provenance and declared custom-domain links without falsely verifying them.

### Acceptance

Playwright against real Rust backend + owned fixtures verifies discover→filter→open/inspect→confirm stop→observe disappearance/respawn, force escalation and reload persistence. Customize a project without repo metadata, reload and verify persistence plus unchanged monitored repo files/status. Screenshots cover desktop/narrow/light/dark; keyboard/focus/reduced motion checked. SSE loss/reconnect and failed mutation show honest UI. Build/lint/types/unit/browser gates pass; no mock-only completion.

## S08

### Scope

Implement tray-icon with documented macOS main-thread event loop and existing async runtime/core, plus the `kosse window` child process (wry + tao WKWebView, focus-or-spawn, browser fallback) and on-demand `kosse open` (server + window, exits with the window; reuses a verified running instance). Native grouped menu, dashboard/open/inspect/stop routing to same confirmed action UI, app-only quit. Default app mode vs headless serve; second launch verifies existing instance and opens it without duplicate scanner. No graphical popover or duplicated action policy.

### Acceptance

Run actual app on macOS: tray visible with owned fixture services, Open Dashboard/Inspect reach the dashboard window routes (and browser fallback), Stop invokes same confirmation, Quit leaves fixtures alive. Headless serves without tray. Second launch no duplicate scanner; unrelated port occupant not treated as trusted instance. Capture automation or explicit human evidence; absent GUI access is waiting, never passed.

## S09

### Scope

Create real release:local/install:local/uninstall:local, binary-first: a macOS arm64 `kosse` executable tarball with embedded UI, checksums, third-party notices (Rust via cargo-about + the Vite license file) and manifest; an unpublished Homebrew formula template that installs it; a minimal .app only if S08 testing showed it is needed. Prefix-aware sandbox tests. Evaluate service-manager/auto-launch; implement startup enable/disable/status, no auto-enable. Implement doctor/version/help, clear exits, config-preserving upgrade and uninstall. License selection/signing/public publishing not assumed.

### Acceptance

Sandbox packaged artifact runs without Node/Bun, serves UI and tray as applicable, discovers fixtures, and uninstalls only owned files; upgrade preserves config. Startup configuration/lifecycle unit/sandbox tests pass; do not modify the maintainer's real login items without approval. Local artifact checksum verified. Verify that a tarball fetched with `curl` runs without a Gatekeeper prompt (no quarantine attribute) and document the browser-download case. Document unsigned limitations and the human login check as separate unverified/approval gates.

## S10

### Scope

Run entire blueprint/verification.md matrix from packaged/installed entrypoints, not only cargo run. Add concise docs/evidence, architecture/command updates, README setup/use/cleanup/limitations, threat model and dependency/advisory/license audit. Validate advertised tray/browser/CLI behavior, fixture teardown and offline runtime. Check all child evidence before closing epic.

### Acceptance

Full local command suite green; installed browser+tray+CLI workflow has evidence, generated contracts current, no uncontrolled fixture processes/files. Required GUI checks performed or explicitly owner-deferred; actual login cycle remains approval-dependent and clearly separated from verified artifact. No public release/remote/push implied. Mark epic done only when required acceptance holds; attach final evidence and remaining limits.

## Next phase

### S07t (next phase, after v1)

Added 2026-10-28 by owner direction: some users don't want a resident app, and terminal users want a first-class client.

Deferred by owner direction (2026-10-28): v1 ships the Rust binary with the dashboard (window or browser) and `kosse open` for on-demand use; the OpenTUI client starts after v1.

#### Scope

`tui/` workspace package on OpenTUI (load its skill and current docs), consuming `web/src/generated/contracts.ts` (shared, never duplicated) and the HTTP/SSE API. `kosse tui` subcommand: attach to a verified running instance (never trust an unverified port occupant), otherwise spawn `kosse serve` privately on a free loopback port for the session and stop it by identity on exit. Feature parity with the dashboard (product.md Terminal surface), keyboard-first, brand palette mapped to terminal colours with a monochrome fallback, narrow-width layout. Compile with `bun build --compile`; measure startup time, RSS and executable size.

#### Acceptance

Real-terminal tests (OpenTUI's test renderer or a PTY harness against the real binary and owned fixtures, scoped with `--probe-only`/`--act-only`) cover discover, filter, inspect, open, stop → still running → force, respawn reporting, customize persistence and server disconnect/reconnect. `kosse tui` with no running instance starts and fully cleans up its private server. Screenshots (fixture data) for the README. Docs and commands updated; check/CI include its lint, types and tests.


## Exit rule

S10 audits all slice evidence before v1 is declared complete. Missing actual GUI/installed-entrypoint verification is not satisfied by mocks or passing unit tests. Do not silently shrink scope to complete a loop. Human-only or privileged actions need explicit approval; local packaging can complete without publishing or signing.
