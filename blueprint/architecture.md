# Architecture and implementation contracts

## Stack and layout

S12 replaces the browser dashboard with a native Rune window (owner direction, 2026-10-29). Historical S01–S11 evidence describes the superseded web implementation.

- Rust stable, Cargo workspace, rustfmt and clippy; pinned tools and lockfiles.
- `crates/core`: discovery, identity, attribution, snapshots, sampling, action policy and configuration. No UI or router dependency.
- `crates/app`: CLI, one scanner scheduler, guarded Axum API/SSE, macOS tray and a native GPU-rendered `kosse window` child.
- Rune and rune-kit are pinned to `b63efb3db3a2fa59bf4a11a947944de2a8b69303`. Rune owns the window, native text input, layout, focus and rendering. rune-kit supplies controls; Kossé supplies project groups, process rows, inspection and central-profile forms.
- The tray retains its tao/muda event loop. The Rune child reads snapshots and sends mutations through the same guarded API. It never scans or signals.
- No web bundle, WebView, JavaScript UI runtime or browser fallback. Bun orchestrates development and verification scripts only. `contracts/contracts.ts` remains generated for those clients.
- Inter UI text, system monospace data and a bundled Kibernetyk Mono wordmark. Neutral light/dark surfaces with Québec-inspired blue; Relay icons have a separate optical menu-bar geometry.
- Cargo integration tests exercise real processes and HTTP. `bun run test:window` drives the actual Rune window through AppKit events. Oxlint/Oxfmt and TypeScript check orchestration scripts.
- The old 50 kB web budget no longer applies. Report native binary size and owner/window CPU/RSS separately. Closing the window releases its GPU runtime.

Small workspace boundaries support reuse without a plugin framework. Future tunnels/proxy integrations remain outside the observational core; see project-metadata.md for provider ownership and consent boundaries. Do not build an ABI/SDK before a concrete integration. Resolve crate APIs from current docs; candidates are not blanket dependency requirements.

## Discovery and sampling

Evaluate `listeners` first for socket→PID mapping against `lsof` ground truth and fixture listeners on macOS. Filter actual TCP LISTEN state; some enumeration APIs expose other sockets. Compare/fallback to `netstat2` only if evidence shows a gap. Use `sysinfo` for cwd, cmd, parent, user, start time, CPU and memory. Do not reimplement Darwin FFI without demonstrated need. S02 outcome: `listeners` 0.6.1 with a small vendored patch (it dropped sockets whose process name is not valid UTF-8); `netstat2` and `libproc` were evaluated and rejected, see docs/evidence/S02.md.

Single shared scan loop, initial target 3-second interval. Coalesce concurrent refresh requests. Retain sampler state across ticks; CPU needs two observations and appropriate refresh timing. Deduplicate process inspections across ports. Serialize scans and bound blocking work away from async request workers. S03 outcome: one dedicated scanner thread in `crates/app` owns the only `Scanner`; CPU is computed per PID + start identity from accumulated CPU time between scans (sysinfo's own `cpu_usage` depends on the refresh of any process and keeps stale values), and sysinfo's process table is rebuilt to the current scan's processes; see docs/evidence/S03.md.

Measure scan duration and idle overhead with actual fixtures. Target under one interval at 30 owned fixture listeners; if missed, report evidence and optimize before increasing default cadence. Unknown permission/process disappearance is partial data, not fatal to the snapshot.

## Model and identity

Define serializable versioned contracts with generated TypeScript types (ts-rs, exported to `contracts/contracts.ts`); a freshness test fails `check` and CI on stale output. UI must not hand-maintain a divergent Rust model.

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

Candidates: automatic probes go only to endpoints on loopback whose process belongs to the user Kossé runs as (effective UID) and is attributed to a project. From S06, the user's central profile (never a repository file) can declare a matched service `http`/`https`, which also makes its own loopback endpoints candidates despite a non-HTTP-looking port or process name; a profile's `probePath` replaces `/` only once a plain `GET /` on that identity and port got an HTTP response. Everything else is `skipped` with a typed reason (`notOnLoopback`, `otherUser`, `knownNonHttpPort`, `knownNonHttpProcess`, `notAttributed`). Other users' and system processes, and non-loopback endpoints, are never probed, not even on request. The user may explicitly ask for one probe ("try HTTP") of any other own-user loopback endpoint through `POST /api/probe` (same guard, token and rate limit as refresh); its result is marked `requested`.

S04 outcome: policy in `crates/core/src/probe.rs`, I/O in `crates/app/src/prober.rs` on hyper's HTTP/1 client connection and rustls (ring provider, TLS 1.2/1.3). Certificates are verified for `localhost` in process by webpki against the roots the user's keychains trust (`rustls-native-certs`, so local development CAs work), with no network access during verification. A server that speaks first gets no request. Redirects are recorded, never followed (so never off loopback). 4 concurrent, 15 s per identity + port; the scanner thread attaches results to the next snapshot. See docs/evidence/S04.md.

## API and browser security

One endpoint, default `127.0.0.1:47831`; configurable explicitly. Refuse accidental non-loopback binding. Fail clearly on collision; do not assume an unrelated occupant is Kossé.

Read endpoints: health/version, session (the action token), snapshot, SSE. Write endpoints: refresh, requested probe, stop, central project profile (S06), the resident app's window (S08), dashboard preferences (`/api/preferences`). All API responses redact before leaving core; do not expose environment variables.

Validate Host against configured loopback authority on every route; `localhost:<port>` is admitted alongside `127.0.0.1:<port>`. For unsafe methods require exact same-origin Origin (reject missing/null/mismatch, and `Sec-Fetch-Site: none`), JSON content type, bounded payload, and a per-instance random action token obtained only through same-origin API. No wildcard CORS. Token is not embedded in URL/logs/persisted config. These defenses target websites/DNS rebinding, not malicious local processes.

The HTTP guard remains active on every route, including the script-free API information page. Native mutations send the exact Host, Origin, JSON content type and fresh per-instance token after verifying socket ownership. Declared links are validated by core, shown with their host and opened only on user action; native clients admit only HTTP(S).

`native/client.rs` owns initial snapshot loading, SSE revisions, reconnect and typed mutations. Only a newer revision or a new instance replaces the snapshot. Search, selection and editing drafts remain local. A disconnected state is visible; ended rows remain in memory for 60 seconds. The wire route forms `/`, `/?inspect=<pid>.<start>` and `/?stop=<pid>.<start>` remain internal owner-to-window commands. They are not browser routes.

“All listeners” remains a persisted central preference. Central profile forms send typed `SaveProfileRequest` data; repository values are read-only and inherited until overridden. Persistence and unknown-field preservation stay in the Rust core.

## Actions

Use typed signal support (sysinfo or narrow OS wrapper) with own-user restriction, self refusal, minimum valid PID, and fresh identity validation. PID/start-time checks reduce reuse risk but are not magically atomic kernel handles: document remaining platform race and minimize check→signal interval.

Signals run only from confirmed action requests against eligible current observations. Re-probe after signal without treating probe errors as success. Do not execute sanitized cmdlines. Do not kill parent/groups automatically.

S05 outcome: policy and wire types in `crates/core/src/action.rs`, the only `kill(2)` in `crates/core/src/signal.rs` (rustix `kill_process`: one positive PID, typed errno; never a group or `-1`), execution in `crates/app/src/actions.rs`.

- `POST /api/stop` (`SIGTERM`) and `POST /api/force` (`SIGKILL`) take `{instance, identity, ports}`. Force is accepted only for an identity whose stop returned `stillRunning` within the offer lifetime, once, and only if the process still had the same start time and parent when the offer was made.
- Before the signal: the instance and force offer; one socket enumeration (the target's live ports, and the other processes already on those port numbers); then one sysinfo refresh of that PID (BSD info only). `revalidate` refuses PID ≤ 1 and Kossé itself before it considers the fresh read (the snapshot's `instancePid` names Kossé's own process, so the dashboard and tray offer no Stop for it; S11), then requires the owner's real and effective UID to equal Kossé's effective UID and the start time to match. The confirmation must name the identity in the current snapshot and exactly its live ports. A PID sysinfo returns nothing for is gone only if `kill(pid, 0)` says `ESRCH` or `ps` reports a zombie; otherwise it is unknown, never gone.
- After the signal: bounded TERM/KILL windows and a respawn grace (constants in `crates/app/src/actions.rs`). Free ports → `exited`; a new owner with the target's parent (PID > 1) → `respawned`; any other new owner → `endpointTaken`; processes that already shared the port numbers before are reported as `shared`, not as successors. An unreadable process or socket enumeration is `observationFailed`, a failed `kill` call `signalFailed`; neither is success. One action runs at a time, at most one per second.

Remaining race (macOS has no pidfd or other process handle for `kill`): between the fresh read and `kill(2)` the process can exit and its PID be reused. The window is that read's end plus in-memory checks (a fresh read takes about 25 µs, release build, docs/evidence/S05.md); XNU allocates PIDs sequentially and wraps at 99,999, so a reuse inside it needs the whole PID space to cycle in microseconds. Start times have one-second resolution, so they distinguish reuse only across seconds; the snapshot's identity (start time) is still compared, and the owner check bounds any mistake to the user's own processes. The same applies after the signal: a target that exits and whose PID is reused within the same second by an own-user process would look still running; the force offer therefore also requires the parent PID seen at revalidation, and force repeats the whole pre-signal check.

## Configuration and packaging

Use platform application directories, versioned JSON and atomic rename. Central project profiles keyed by canonical worktree path are the default; repository files are optional read-only inputs unless explicitly exported. UI customization has zero repository footprint. Implement effective-profile precedence and provenance per project-metadata.md. Keep permissions restrictive for private metadata. No daemon token in config. Display name Kossé; machine-facing names are ASCII `kosse`, and the macOS bundle identifier (when an .app is packaged) is `com.davidpp.kosse`.

S06 outcome: `crates/core/src/profiles`. The central file is `~/Library/Application Support/Kosse/config.json` (`KOSSE_CONFIG_DIR` overrides the directory), written under an advisory lock with temp file + `fsync` + rename (dir `0700`, file `0600`; a symbolic link stays a link and its target is replaced). Profiles apply to each observation on the scanner thread and appear in the snapshot (`Observation.profiles`, `Service.profile`); "Customize this project" is `POST /api/profile`. See docs/evidence/S06.md.

Required executable command contract (implement, test, then document exact syntax):
- `kosse`: resident app with tray + server, opens dashboard on explicit initial launch.
- `kosse open`: on-demand dashboard without a resident app: starts the server (or reuses a verified running instance), opens the dashboard window, and exits with it.
- `kosse tui` (next phase): on-demand terminal client; attaches to a verified running instance or starts a private server for its lifetime.
- `kosse serve --no-open --port <n>`: foreground headless.
- `kosse list --json`: daemon snapshot when compatible instance exists; one-shot discovery otherwise.
- `kosse doctor`: dependency/platform/config/instance diagnostics, non-destructive.
- `kosse startup enable|disable|status`: opt-in user login management; evaluate service-manager/auto-launch instead of bespoke cross-platform templates.
- `--version`, `--help`; clear exit codes; shutdown cleans app resources, not observed services.
A second launch should open the verified existing instance rather than duplicate sampling. Never trust a PID file alone.

The tray owns the main thread in the resident process. Rune owns the main thread in the window child. The owner sends `<url>\t<title>` commands over stdin; EOF closes the child, including when the owner is killed. A closed/crashed child can be recreated without restarting the scanner.

Every owner mode can handle guarded `POST /api/window`: resident, foreground (`serve`) and temporary (`open`). Repeated opens reuse its existing child. Starting a resident app over a temporary owner is refused because that owner's lifetime remains tied to its window. Instance reuse still checks socket ownership and API identity first.

A native window failure is reported in stderr and the tray. A temporary owner exits with failure and releases its listener; no browser is launched. Service Open actions still use the default browser. AppKit activation is cooperative; focus alongside other applications remains a desktop check.

Local release = macOS arm64 archive containing the `kosse` executable (Rune UI, tray and window support built in), checksums, third-party notices and install/uninstall instructions. No Node/Bun required at runtime. Distribution is binary-first (owner direction, 2026-10-28): a plain executable installed by `curl`/tarball or a Homebrew tap formula (later possibly `cargo install`; the crates are `publish = false` today) is not quarantined by Gatekeeper, so it runs without signing or notarization (Apple silicon's mandatory signature is the linker's ad-hoc one). Tray, window and login startup (a LaunchAgent pointing at the binary) all work from a bare executable; a minimal `.app` bundle is optional and only added if real testing shows the tray or window needs it. Signing/notarization/public distribution (including publishing a tap) remain separate approval-dependent work.

S09 outcome (see docs/evidence/S09.md): `bun run release:local` builds `kosse-<version>-aarch64-apple-darwin.tar.gz` (the `cargo auditable` release binary, `LICENSE`, cargo-about and bundled-font notices, `INSTALL.md`, `MANIFEST.json` with per-file SHA-256), its `.sha256` and an unpublished formula; `install:local`/`uninstall:local` are prefix-aware and own exactly the files their install record lists; the configuration is never touched except by an explicit `--purge`. No `.app` bundle (S08's bare-executable tray and window sufficed; the human checklist still applies). Login startup is a per-user LaunchAgent written by `crates/app/src/startup.rs` (`RunAtLoad`, `Aqua` only, no `KeepAlive`), loaded with `launchctl bootstrap`/`bootout gui/<uid>`: `auto-launch` never loads its agent and `service-manager` uses the legacy `load`/`unload`, and neither lets tests redirect the agents directory. `kosse doctor` only reads (the port check is the same owner-then-API verification as a second launch).

## Developer command contract

Root commands are documented in CONTRIBUTING.md. `bun run check` is the non-mutating format, lint, type, Rust test, contract-freshness and release-build gate. `bun run test:window` separately requires an unlocked macOS graphical session. `bun run app` deliberately opens the real app on owned fixtures. `release:local`, `install:local`, `uninstall:local` and `test:package` retain prefix-aware packaging; they never publish or enable startup.
