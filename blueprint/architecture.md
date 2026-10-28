# Architecture and implementation contracts

## Stack and layout

Scaffolded in S01 with current supported generators; versions and rationale are in docs/evidence/S01.md, tool pins in `mise.toml` and `rust-toolchain.toml`, lockfiles `Cargo.lock` and `bun.lock`.

- Rust stable, Cargo workspace, rustfmt + clippy. Pin a tested toolchain in rust-toolchain.toml.
- `crates/core`: listener discovery, process identity, attribution, snapshots, sampling, action policy, config. No UI/router dependency.
- `crates/app`: CLI, one scanner scheduler, Axum API/SSE, embedded web assets, macOS tray lifecycle.
- `web`: Bun, Vite, React strict TypeScript, TanStack Router file routes, TanStack Query for API state, Tailwind v4, shadcn/ui via CLI, Lucide.
- Oxlint/Oxfmt; Lefthook. Vitest + Testing Library for UI logic; Playwright for browser integration. Cargo unit/integration tests.
- Framer Motion only if animation is useful; React Hook Form + Zod only when forms warrant them. Do not add unused dependencies.
- No TanStack Start, Workers or database: Rust serves a local SPA/API, not a hosted full-stack app.

Small workspace boundaries support reuse without a plugin framework. Future tunnels/proxy integrations remain outside the observational core; see project-metadata.md for provider ownership and consent boundaries. Do not build an ABI/SDK before a concrete integration. Resolve crate APIs from current docs; candidates are not blanket dependency requirements.

## Discovery and sampling

Evaluate `listeners` first for socket→PID mapping against `lsof` ground truth and fixture listeners on macOS. Filter actual TCP LISTEN state; some enumeration APIs expose other sockets. Compare/fallback to `netstat2` only if evidence shows a gap. Use `sysinfo` for cwd, cmd, parent, user, start time, CPU and memory. Do not reimplement Darwin FFI without demonstrated need.

Single shared scan loop, initial target 3-second interval. Coalesce concurrent refresh requests. Retain sampler state across ticks; CPU needs two observations and appropriate refresh timing. Deduplicate process inspections across ports. Serialize scans and bound blocking work away from async request workers.

Measure scan duration and idle overhead with actual fixtures. Target under one interval at 30 owned fixture listeners; if missed, report evidence and optimize before increasing default cadence. Unknown permission/process disappearance is partial data, not fatal to the snapshot.

## Model and identity

Define serializable versioned contracts with generated TypeScript types (ts-rs, exported to `web/src/generated/contracts.ts`); a freshness test fails `check` and CI on stale output. UI must not hand-maintain a divergent Rust model.

- ProcessIdentity: PID + process start timestamp; metadata may be unavailable. Identity without start evidence is not eligible for destructive action.
- ProcessRecord: identity, sanitized command, cwd, parent PID, user/ownership, age, metrics availability.
- Endpoint: protocol, bound address, port, observed timestamp, optional HTTP probe result and URL confidence.
- Service: process identity, endpoints, project attribution, package label, observation status.
- Project: stable canonical worktree path identity, repository/worktree root, display name, branch, optional package path.
- Snapshot: schema version, monotonic revision, scan time/duration, last successful observation, services/projects, warnings.
- Action result: structured success/conflict/permission/unsupported/still-running/respawned/observation-error; never raw shell strings as protocol.

Normalize IPv4/IPv6 duplicates for presentation while retaining bind/exposure evidence. Group under repo/worktree root, retain nearest manifest as a package label. Do not merge separate worktrees by common Git directory. Stop ancestor walks at user-home/filesystem boundary. Resolve Git via supported Git command/library behavior, not assumptions that .git is always a directory. Use timeouts and cache metadata; fixture-test detached HEAD and .git files.

## HTTP enrichment

Probe candidate HTTP endpoints only; do not blast every database with HTTP. Small concurrency, connect/total timeout, bounded streaming body consumption (e.g. 64 KiB), no automatic redirects outside the original loopback endpoint. Avoid externally bound addresses as probe destinations; derive safe loopback URLs, handle IPv6-only listeners. Expose probe freshness. No arbitrary user-supplied URL fetch endpoint. Default probe cadence slower than socket scans and cache by process identity + endpoint.

## API and browser security

One endpoint, default `127.0.0.1:47831`; configurable explicitly. Refuse accidental non-loopback binding. Fail clearly on collision; do not assume an unrelated occupant is Stackscan.

Read endpoints: health/version, snapshot, SSE. Write endpoints: refresh, stop, preferences as required. All API responses redact before leaving core; do not expose environment variables.

Validate Host against configured loopback authority on every route. For unsafe methods require exact same-origin Origin (reject missing/null/mismatch), JSON content type, bounded payload, and a per-instance random action token obtained only through same-origin API. No wildcard CORS. Token is not embedded in URL/logs/persisted config. These defenses target websites/DNS rebinding, not malicious local processes.

CSP and escaped text; no remote scripts/fonts. Test malicious Host, Origin, preflight, cross-site forms, missing token, stale token and invalid payload through real HTTP entrypoints. Dev uses a same-origin Vite proxy to the backend: Vite on `127.0.0.1:47833` proxies `/api` to the backend on `127.0.0.1:47832` (both defined in `web/dev-ports.ts`, away from the installed default 47831). The Host/Origin allowlist admits exactly that dev authority only when the backend runs in dev mode, with no production relaxation.

TanStack Query owns client server-state cache. SSE updates/invalidation flow through it; no competing polling store. Reconnect with backoff and fresh snapshot; monotonic revisions handle dropped/out-of-order events. Disconnected/stale is visible.

## Actions

Use typed signal support (sysinfo or narrow OS wrapper) with own-user restriction, self refusal, minimum valid PID, and fresh identity validation. PID/start-time checks reduce reuse risk but are not magically atomic kernel handles: document remaining platform race and minimize check→signal interval.

Signals run only from confirmed action requests against eligible current observations. Re-probe after signal without treating probe errors as success. Do not execute sanitized cmdlines. Do not kill parent/groups automatically.

## Configuration and packaging

Use platform application directories, versioned JSON and atomic rename. Central project profiles keyed by canonical worktree path are the default; repository files are optional read-only inputs unless explicitly exported. UI customization has zero repository footprint. Implement effective-profile precedence and provenance per project-metadata.md. Keep permissions restrictive for private metadata. No daemon token in config.

Required executable command contract (implement, test, then document exact syntax):
- `stackscan`: resident app with tray + server, opens dashboard on explicit initial launch.
- `stackscan serve --no-open --port <n>`: foreground headless.
- `stackscan list --json`: daemon snapshot when compatible instance exists; one-shot discovery otherwise.
- `stackscan doctor`: dependency/platform/config/instance diagnostics, non-destructive.
- `stackscan startup enable|disable|status`: opt-in user login management; evaluate service-manager/auto-launch instead of bespoke cross-platform templates.
- `--version`, `--help`; clear exit codes; shutdown cleans app resources, not observed services.
A second launch should open the verified existing instance rather than duplicate sampling. Never trust a PID file alone.

Native tray requires correct macOS main-thread event-loop integration. Evaluate tray-icon plus its supported event loop; package a minimal .app where needed. No Electron/Tauri rewrite merely to get a menu. All views talk to the same core/snapshot.

Local release = macOS arm64 archive containing executable/app as needed, embedded frontend, checksums, third-party notices and install/uninstall instructions. No Node/Bun required at runtime. Signing/notarization/public distribution are separate approval-dependent work.

## Developer command contract

Scaffold must choose one documented root command runner (Bun scripts may orchestrate web and Cargo without making Rust depend on JS at runtime):
`bun run dev`, `build`, `check`, `test`, `test:e2e`, `fmt`, `fmt:check`, `release:local`, `install:local`, `uninstall:local`.

- check: non-mutating Rust fmt/clippy/tests, web format/lint/type/tests, contract freshness and production build.
- test:e2e: real backend + owned fixtures + browser, isolated temp config and ports.
- release:local: versioned artifact + manifest/checksums, no publishing.
- install/uninstall: prefix-aware sandbox support; do not touch live installation or startup as a test side effect.
- hooks: fast non-mutating pre-commit gate; CI full checks and macOS fixture/build lane. No hooks that silently rewrite/stage code.
Supporting commands also exist: `lint`, `typecheck`, `contracts` (regenerate TS contracts) and `audit` (cargo-deny + workflow lint). `test:e2e` arrives with the browser slice (S07) and the release/install commands with packaging (S09).
Configure shell child teardown for dev and tests. Add reproducible lockfiles and documented tool versions. No pretend-green placeholder commands; add a command only when its behavior exists.
