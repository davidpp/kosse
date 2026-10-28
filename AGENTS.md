# Kossé — agent instructions

Kossé observes local development services (listeners, processes, projects) and offers a browser dashboard and macOS tray menu over one Rust core. Contracts live in [`blueprint/`](blueprint/README.md); read the product, architecture and verification documents before changing behaviour, and the [implementation plan](blueprint/implementation-plan.md) slice you are working on. Commands and setup are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Commands

- `bun run check` — the full non-mutating gate (CI runs the same; the pre-commit hook runs a fast subset). Run it before claiming work is done.
- `bun run dev` — backend (`serve --dev`, debug builds only) on `127.0.0.1:47832`, Vite on `127.0.0.1:47833` with a same-origin `/api` proxy.
- `cargo nextest run --workspace <filter>` runs matching Rust tests (`--workspace` also builds the `kosse-fixture` binary that fixture tests spawn; with `-p` alone they fail on a clean target, or run a stale fixture binary on a warm one); `bun run --cwd web test <file-pattern>` runs matching web test files.
- `bun run contracts` regenerates `web/src/generated/contracts.ts` after a contract type changes. Never hand-edit it.
- `bun run budget` checks `web/dist` against the 50 kB gzipped JS + CSS budget (no web fonts); `test` and `build` run it after `vite build`.
- `bun run fmt` and `bun run contracts` rewrite tracked files. CI fails if `check` leaves the tree dirty.

## Invariants

- One scanner per running application. Views, request handlers and the tray read the shared snapshot; they never scan.
- All views use the same service model and action policy. No alternate destructive path.
- A destructive action needs a fresh PID + start-time identity and explicit confirmation of every port. Never signal process groups, parents, other users' processes, or Kossé itself. The only `kill` is `crates/core/src/signal.rs`, which takes only an `action::Eligible` from `action::revalidate`; force needs a prior `stillRunning`. Test servers run with the debug-only `--act-only=<fixture PIDs>`.
- Bind loopback only. Every route passes `crates/app/src/guard.rs` (exact Host, cross-origin refusal, unsafe-method Origin/JSON/size/token checks, CSP); never relax it for development beyond the `--dev` authority, which exists only in debug builds. No CORS headers.
- Every displayed, served or logged command line passes `kosse_core::redact` (applied once, in `inspect.rs`); raw argv never leaves that module. Process- and probe-derived strings are untrusted text. Request handlers log nothing per request.
- HTTP probes only connect to loopback addresses (never a resolved name), never follow redirects, cap time and body, and never disable TLS verification (rustls, offline). Automatic probes go only to own-user, project-attributed processes; other users' processes are never probed. The only URLs emitted are `prober::open_url`'s `http(s)://localhost:<port>/`. Tests never probe real services: `serve` in tests runs with the debug-only `--probe-only=<fixture PIDs>`.
- `crates/core` has no UI, router or HTTP-server dependency.
- Socket enumeration, process name and executable path come from `listeners` (patched, vendored in `vendor/listeners`) only inside `crates/core/src/sockets.rs`; start time, parent, user, command line (redacted there) and cwd come from `sysinfo` in `crates/core/src/inspect.rs`. Never drop a socket for missing metadata: unknown is `null` plus a snapshot warning, never a silent omission or an empty success.
- Project attribution never parses `.git` or manifest contents itself: a stat-only walk (stopping before the home directory, the filesystem root or another filesystem) finds candidates, and `git` runs only through `crates/core/src/git.rs` (developer-directory binary, timeout, no inherited `GIT_*`). Separate worktrees are separate projects; an unknown cwd is no project, never a guess.
- CPU is never a fabricated zero: `warmingUp` until a second observation of the same PID + start identity, `unavailable` when withheld.
- Rust: expected failures are typed `Result`s; no `unwrap`/`expect`/`panic` on OS data or process races. Use `#[expect(lint, reason = "…")]`, not `#[allow]`.
- TypeScript: strict, no `any`, `@ts-ignore` or unchecked casts (generated files excepted). The runtime is TanStack Redact, not React; import from `react` as usual. TanStack Query owns server state (snapshot + SSE by revision); components never fetch outside it. Stay within the bundle budget in blueprint/architecture.md.
- Small modules, no speculative traits or plugin layers. Add a dependency only when code uses it.

## Tests and evidence

- Test real behaviour through real entrypoints: owned fixture processes (`crates/fixtures`, checked against `lsof`), the real HTTP server, a real browser. Mocks are only acceptable for pure domain logic or platform-provided test doubles.
- Tests never stop a process they did not start. Fixtures are recorded and cleaned up by identity, never by process name.
- A slice is done when its acceptance holds through the user-facing entrypoint. Record commands, results and anything unverified in `docs/evidence/<slice>.md`. Never mark a human-only check (tray on a real desktop, login startup) as passed.
- Do not weaken an assertion to get green; fix the cause or report the blocker.

## Boundaries

- Do not push, tag, publish, sign, install login items, or change global tool/agent configuration unless the maintainer asks.
- Keep public artifacts clean: no private absolute paths, personal tracker IDs, real process command lines, secrets, or conversation history in code, commits or docs. Plan slice IDs (`S04`) are fine.
- If implementation must change a blueprint contract, update the smallest relevant section and state why.
- Provenance: Kossé is independently written from its blueprint and public library documentation. Do not consult or translate code from other port/process dashboards (e.g. PortDoc); see [blueprint/references.md](blueprint/references.md).
- UI work follows the design context in the web app and uses the Impeccable skill when available.
