# Architecture and implementation contracts

## Stack and layout

Scaffolded in S01 with current supported generators; versions and rationale are in docs/evidence/S01.md, tool pins in `mise.toml` and `rust-toolchain.toml`, lockfiles `Cargo.lock` and `bun.lock`.

- Rust stable, Cargo workspace, rustfmt + clippy. Pin a tested toolchain in rust-toolchain.toml.
- `crates/core`: listener discovery, process identity, attribution, snapshots, sampling, action policy, config. No UI/router dependency.
- `crates/app`: CLI, one scanner scheduler, Axum API/SSE, embedded web assets, macOS tray lifecycle, and the dashboard window (a `kosse window` child process: wry + tao WKWebView on the loopback URL).
- `tui`: the first-party terminal client in TypeScript on [OpenTUI](https://github.com/anomalyco/opentui) (React bindings, real React: Redact targets the DOM), consuming the same generated contracts and HTTP/SSE API as the dashboard. Built with `bun build --compile` into a separate `kosse-tui` executable shipped next to `kosse`; `kosse tui` finds and runs it, so the core binary stays small and needs no Bun. It sends the same Host/Origin/token headers as a same-origin page.
- `web`: Bun, Vite, React APIs on the [TanStack Redact](https://tanstack.com/redact/latest) runtime (Vite plugin; synchronous rendering, ~20 kB gzipped instead of React's ~69 kB), strict TypeScript, TanStack Query for server state, Tailwind v4 (build-time only), Lucide icons imported per icon, system font stack, native `<dialog>`. Hand-built components designed with Impeccable; no component framework. Redact is pre-1.0 and adopted deliberately (owner choice); pin exact versions and re-verify on upgrade. Outcome of S01b: TanStack Router measured (~28 kB; 58.6 kB total with it vs ~31 kB without) and not adopted; addressable views (inspect, window routes for S08) use a minimal location/history switch (S07). Redact hydration/activity/fragmentRefs off; Vitest runs the Redact runtime (`server.deps.inline` + client `mainFields`); `check` enforces the budget via `bun run budget`.
- Performance budget (owner direction, 2026-10-28: the tool must be nimble): shipped JS + CSS ≤ 50 kB gzipped in total, no web fonts; `check` fails when the budget is exceeded. The resident app's idle cost is measured and reported in evidence (CPU, RSS with the window closed and open).
- Oxlint/Oxfmt; Lefthook. Vitest + Testing Library for UI logic; Playwright for browser integration. Cargo unit/integration tests.
- CSS transitions only where motion helps comprehension, respecting reduced motion; no animation or form libraries. Do not add unused dependencies.
- [Rune](https://github.com/anomalyco/rune) (Rust GPU UI) is tracked as a candidate for a future native window/popover once it has releases, status-item support and API stability; it is not a v1 dependency.
- No full-stack framework, Workers or database: Rust serves a small local SPA/API, not a hosted app.

Small workspace boundaries support reuse without a plugin framework. Future tunnels/proxy integrations remain outside the observational core; see project-metadata.md for provider ownership and consent boundaries. Do not build an ABI/SDK before a concrete integration. Resolve crate APIs from current docs; candidates are not blanket dependency requirements.

## Discovery and sampling

Evaluate `listeners` first for socket→PID mapping against `lsof` ground truth and fixture listeners on macOS. Filter actual TCP LISTEN state; some enumeration APIs expose other sockets. Compare/fallback to `netstat2` only if evidence shows a gap. Use `sysinfo` for cwd, cmd, parent, user, start time, CPU and memory. Do not reimplement Darwin FFI without demonstrated need. S02 outcome: `listeners` 0.6.1 with a small vendored patch (it dropped sockets whose process name is not valid UTF-8); `netstat2` and `libproc` were evaluated and rejected, see docs/evidence/S02.md.

Single shared scan loop, initial target 3-second interval. Coalesce concurrent refresh requests. Retain sampler state across ticks; CPU needs two observations and appropriate refresh timing. Deduplicate process inspections across ports. Serialize scans and bound blocking work away from async request workers. S03 outcome: one dedicated scanner thread in `crates/app` owns the only `Scanner`; CPU is computed per PID + start identity from accumulated CPU time between scans (sysinfo's own `cpu_usage` depends on the refresh of any process and keeps stale values), and sysinfo's process table is rebuilt to the current scan's processes; see docs/evidence/S03.md.

Measure scan duration and idle overhead with actual fixtures. Target under one interval at 30 owned fixture listeners; if missed, report evidence and optimize before increasing default cadence. Unknown permission/process disappearance is partial data, not fatal to the snapshot.

## Model and identity

Define serializable versioned contracts with generated TypeScript types (ts-rs, exported to `web/src/generated/contracts.ts`); a freshness test fails `check` and CI on stale output. UI must not hand-maintain a divergent Rust model.

- ProcessIdentity: PID + process start timestamp; metadata may be unavailable. Identity without start evidence is not eligible for destructive action.
- ProcessRecord: identity, sanitized command (`command`: redacted `args`, plus `redacted` and `truncated` flags), cwd, parent PID, user/ownership, age, metrics availability.
- Endpoint: protocol, port, every canonical bind address on that port, exposure (loopback/interface/allInterfaces, from binds alone), HTTP probe state and URL confidence; observation time is the snapshot's `scannedAtMs`. Probe state (`http`): `skipped` (with reason), `pending`, or `probed` (`probedAtMs`, `requested`, and a result: `response` with URL, status, title, recorded redirect `Location`, TLS and body-truncated; `timedOut`, `refused`, `closed`, `tlsFailed` with certificate/handshake failure, `notHttp`). URL confidence = probe state: `response` means an HTTP server answered on that loopback URL; declared links arrive with S06 profiles.
- Service: process identity, endpoints, project attribution, package label, observation status.
- Project: canonical worktree root (identity; the package directory outside Git), display name, Git facts (common directory for display grouping only, linked worktree, branch or detached HEAD; `null` outside Git). The package label (path relative to the project root, name, manifests) is on the Service.
- Snapshot: schema version, monotonic revision, last successful observation (scan time/duration, services/projects, warnings; `null` before the first success), scanner status (interval, last attempt, failure, consecutive failures, overruns).
- Action result: structured success/conflict/permission/unsupported/still-running/respawned/observation-error; never raw shell strings as protocol.

Normalize IPv4/IPv6 duplicates for presentation while retaining bind/exposure evidence. Group under repo/worktree root, retain nearest manifest as a package label. Do not merge separate worktrees by common Git directory. Stop ancestor walks at user-home/filesystem boundary. Resolve Git via supported Git command/library behavior, not assumptions that .git is always a directory. Use timeouts and cache metadata; fixture-test detached HEAD and .git files. S03 uses the `git` CLI from the active developer directory (`xcode-select -p`, never the `/usr/bin/git` shim that prompts to install tools), with a 2 s timeout, only for directories whose stat-only walk found a `.git` entry; layouts are cached 60 s and branches 15 s.

## HTTP enrichment

Probe candidate HTTP endpoints only; do not blast every database with HTTP. Small concurrency, connect/total timeout, bounded streaming body consumption (e.g. 64 KiB), no automatic redirects outside the original loopback endpoint. Avoid externally bound addresses as probe destinations; derive safe loopback URLs, handle IPv6-only listeners. Expose probe freshness. No arbitrary user-supplied URL fetch endpoint. Default probe cadence slower than socket scans and cache by process identity + endpoint.

Candidates: automatic probes go only to endpoints on loopback whose process belongs to the user Kossé runs as (effective UID) and is attributed to a project (or, from S06, declared HTTP by a profile). Everything else is `skipped` with a typed reason (`notOnLoopback`, `otherUser`, `knownNonHttpPort`, `knownNonHttpProcess`, `notAttributed`). Other users' and system processes, and non-loopback endpoints, are never probed, not even on request. The user may explicitly ask for one probe ("try HTTP") of any other own-user loopback endpoint through `POST /api/probe` (same guard, token and rate limit as refresh); its result is marked `requested`.

S04 outcome: policy in `crates/core/src/probe.rs`, I/O in `crates/app/src/prober.rs` on hyper's HTTP/1 client connection and rustls (ring provider, TLS 1.2/1.3). Certificates are verified for `localhost` in process by webpki against the roots the user's keychains trust (`rustls-native-certs`, so local development CAs work), with no network access during verification. A server that speaks first gets no request. Redirects are recorded, never followed (so never off loopback). 4 concurrent, 15 s per identity + port; the scanner thread attaches results to the next snapshot. See docs/evidence/S04.md.

## API and browser security

One endpoint, default `127.0.0.1:47831`; configurable explicitly. Refuse accidental non-loopback binding. Fail clearly on collision; do not assume an unrelated occupant is Kossé.

Read endpoints: health/version, session (the action token), snapshot, SSE. Write endpoints: refresh, requested probe, stop, preferences as required. All API responses redact before leaving core; do not expose environment variables.

Validate Host against configured loopback authority on every route; `localhost:<port>` is admitted alongside `127.0.0.1:<port>`. For unsafe methods require exact same-origin Origin (reject missing/null/mismatch, and `Sec-Fetch-Site: none`), JSON content type, bounded payload, and a per-instance random action token obtained only through same-origin API. No wildcard CORS. Token is not embedded in URL/logs/persisted config. These defenses target websites/DNS rebinding, not malicious local processes.

CSP and escaped text; no remote scripts/fonts. Redact, unlike React 19, does not neutralize `javascript:` URLs in `href`, so every link the UI renders from process-derived or configured data goes through one link builder that accepts only `http:`/`https:` (S07); CSP is the backstop, not the guard. S04 outcome: `crates/app/src/guard.rs` applies the checks to every route (the embedded bundle included), refuses API requests a browser marks cross-origin (`Origin` or `Sec-Fetch-Site`), takes the token from the `X-Kosse-Token` header only, rate-limits refresh and requested probes to one per second, and the dev authority exists only in debug builds (`serve --dev`). Test malicious Host, Origin, preflight, cross-site forms, missing token, stale token and invalid payload through real HTTP entrypoints. Dev uses a same-origin Vite proxy to the backend: Vite on `127.0.0.1:47833` proxies `/api` to the backend on `127.0.0.1:47832` (both defined in `web/dev-ports.ts`, away from the installed default 47831). The Host/Origin allowlist admits exactly that dev authority only when the backend runs in dev mode, with no production relaxation.

TanStack Query owns client server state: an initial snapshot fetch, then SSE updates written into the query cache only when their revision is newer; no competing polling store. Mutations go through one typed client module. Reconnect with backoff and fresh snapshot; monotonic revisions handle dropped/out-of-order events. Disconnected/stale is visible.

## Actions

Use typed signal support (sysinfo or narrow OS wrapper) with own-user restriction, self refusal, minimum valid PID, and fresh identity validation. PID/start-time checks reduce reuse risk but are not magically atomic kernel handles: document remaining platform race and minimize check→signal interval.

Signals run only from confirmed action requests against eligible current observations. Re-probe after signal without treating probe errors as success. Do not execute sanitized cmdlines. Do not kill parent/groups automatically.

S05 outcome: policy and wire types in `crates/core/src/action.rs`, the only `kill(2)` in `crates/core/src/signal.rs` (rustix `kill_process`: one positive PID, typed errno; never a group or `-1`), execution in `crates/app/src/actions.rs`.

- `POST /api/stop` (`SIGTERM`) and `POST /api/force` (`SIGKILL`) take `{instance, identity, ports}`. Force is accepted only for an identity whose stop returned `stillRunning` within the offer lifetime, once, and only if the process still had the same start time and parent when the offer was made.
- Before the signal: the instance and force offer; one socket enumeration (the target's live ports, and the other processes already on those port numbers); then one sysinfo refresh of that PID (BSD info only). `revalidate` refuses PID ≤ 1 and Kossé itself before it considers the fresh read, then requires the owner's real and effective UID to equal Kossé's effective UID and the start time to match. The confirmation must name the identity in the current snapshot and exactly its live ports. A PID sysinfo returns nothing for is gone only if `kill(pid, 0)` says `ESRCH` or `ps` reports a zombie; otherwise it is unknown, never gone.
- After the signal: bounded TERM/KILL windows and a respawn grace (constants in `crates/app/src/actions.rs`). Free ports → `exited`; a new owner with the target's parent (PID > 1) → `respawned`; any other new owner → `endpointTaken`; processes that already shared the port numbers before are reported as `shared`, not as successors. An unreadable process or socket enumeration is `observationFailed`, a failed `kill` call `signalFailed`; neither is success. One action runs at a time, at most one per second.

Remaining race (macOS has no pidfd or other process handle for `kill`): between the fresh read and `kill(2)` the process can exit and its PID be reused. The window is that read's end plus in-memory checks (a fresh read takes about 25 µs, release build, docs/evidence/S05.md); XNU allocates PIDs sequentially and wraps at 99,999, so a reuse inside it needs the whole PID space to cycle in microseconds. Start times have one-second resolution, so they distinguish reuse only across seconds; the snapshot's identity (start time) is still compared, and the owner check bounds any mistake to the user's own processes. The same applies after the signal: a target that exits and whose PID is reused within the same second by an own-user process would look still running; the force offer therefore also requires the parent PID seen at revalidation, and force repeats the whole pre-signal check.

## Configuration and packaging

Use platform application directories, versioned JSON and atomic rename. Central project profiles keyed by canonical worktree path are the default; repository files are optional read-only inputs unless explicitly exported. UI customization has zero repository footprint. Implement effective-profile precedence and provenance per project-metadata.md. Keep permissions restrictive for private metadata. No daemon token in config. Display name Kossé; machine-facing names are ASCII `kosse`, and the macOS bundle identifier (when an .app is packaged) is `com.davidpp.kosse`.

Required executable command contract (implement, test, then document exact syntax):
- `kosse`: resident app with tray + server, opens dashboard on explicit initial launch.
- `kosse open`: on-demand dashboard without a resident app: starts the server (or reuses a verified running instance), opens the dashboard window, and exits with it.
- `kosse tui`: on-demand terminal client; attaches to a verified running instance or starts a private server for its lifetime.
- `kosse serve --no-open --port <n>`: foreground headless.
- `kosse list --json`: daemon snapshot when compatible instance exists; one-shot discovery otherwise.
- `kosse doctor`: dependency/platform/config/instance diagnostics, non-destructive.
- `kosse startup enable|disable|status`: opt-in user login management; evaluate service-manager/auto-launch instead of bespoke cross-platform templates.
- `--version`, `--help`; clear exit codes; shutdown cleans app resources, not observed services.
A second launch should open the verified existing instance rather than duplicate sampling. Never trust a PID file alone.

Native tray requires correct macOS main-thread event-loop integration. Evaluate tray-icon plus its supported event loop; package a minimal .app where needed. The dashboard window runs as a child process (`kosse window`) so a WebKit crash or hang cannot take down the scanner or tray, and closing it frees the web engine's memory; it loads the same loopback origin as a browser, so the HTTP security rules apply unchanged. If the window cannot start, open the default browser and say why. Any browser can still open the dashboard URL. No Electron/Tauri rewrite merely to get a menu. All views talk to the same core/snapshot.

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
