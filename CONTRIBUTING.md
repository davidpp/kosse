# Contributing

Stackscan is built in vertical slices defined in [`blueprint/implementation-plan.md`](blueprint/implementation-plan.md). Each slice lands with tests through real entrypoints and a short evidence note in [`docs/evidence/`](docs/evidence/). AI-assisted contributions are welcome under the same bar: include the commands you ran and their results.

## Prerequisites

- macOS on Apple silicon (the only supported and tested v1 platform) with Xcode Command Line Tools (`xcode-select --install`) for the linker.
- Tests spawn short-lived owned listener processes on loopback, plus one on the `[::]` wildcard, and call `/usr/sbin/lsof` (ground truth) and `/usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate`. The wildcard test is skipped when the application firewall is on, so it never triggers an "accept incoming connections" prompt.
- Tests never probe or otherwise contact real services: every `stackscan serve` a test starts runs with the debug-only `--probe-only=<PIDs>`, empty by default, so only the test's own fixtures are probed. The one exception to "owned ports only" is the default-port test: it binds `127.0.0.1:47831` to occupy it, or, if something already listens there, only connects to it (never sends data or signals).
- Attribution tests build temporary Git repositories with `git` from the active developer directory (`$(xcode-select -p)/usr/bin/git`, the same binary Stackscan runs), ignoring global and system Git config and any inherited `GIT_*` variables.
- [mise](https://mise.jdx.dev), which pins every tool per repository in [`mise.toml`](mise.toml): Rust (also pinned in `rust-toolchain.toml`), Bun, Node (needed only by web test tooling), cargo-nextest, cargo-deny and zizmor.

```sh
mise trust && mise install   # once per machine
bun install --frozen-lockfile  # also installs the repo-local Lefthook hook
bun run check
```

Installing needs network access (mise tool downloads, `bun install`, `cargo fetch`, and the advisory database for `bun run audit`). The built `stackscan` binary embeds the web UI and needs neither Bun nor Node at runtime.

## Commands

| Command                     | Behavior                                                                                                                                                                                                                                                                                                                                                    |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bun run dev`               | Builds the debug backend and runs it with `--dev` on `127.0.0.1:47832` plus Vite on `127.0.0.1:47833`. Open `http://127.0.0.1:47833/`; Vite proxies `/api` to the backend (same origin, development only). Ctrl-C stops both; if either exits, the other is stopped.                                                                                        |
| `bun run build`             | `vite build` into `web/dist`, `budget`, then `cargo build --release`, embedding the bundle in `target/release/stackscan`.                                                                                                                                                                                                                                   |
| `bun run check`             | Non-mutating full gate, identical to CI: `fmt:check`, `lint`, `typecheck`, `test`, release build.                                                                                                                                                                                                                                                           |
| `bun run test`              | Vitest on the Redact runtime, web build, `budget`, `cargo nextest run --workspace` (binary-level server, API security, event stream, HTTP probe and redaction tests; discovery, attribution, metrics, scheduler and `stackscan list` tests against owned fixture processes, temporary Git repositories and `lsof`; contract freshness), `cargo test --doc`. |
| `bun run lint`              | `lint:web` (`oxlint --deny-warnings`) and `cargo clippy --all-targets -- -D warnings`.                                                                                                                                                                                                                                                                      |
| `bun run typecheck`         | `tsc --noEmit` for `web/` and `scripts/`.                                                                                                                                                                                                                                                                                                                   |
| `bun run fmt` / `fmt:check` | `cargo fmt` + `oxfmt` (write / check).                                                                                                                                                                                                                                                                                                                      |
| `bun run budget`            | Fails when the gzipped JS + CSS in `web/dist` exceeds 50 kB, or the bundle carries a web font (font file or `@font-face`). Prints per-file gzip and raw sizes.                                                                                                                                                                                              |
| `bun run contracts`         | Regenerates `web/src/generated/contracts.ts` from the Rust contract types. A stale file fails `test` and `check`.                                                                                                                                                                                                                                           |
| `bun run audit`             | `cargo deny check` (advisories, licenses, bans, sources) and `zizmor` over the workflows. Needs network for the advisory database; runs as a separate CI job.                                                                                                                                                                                               |

The pre-commit hook ([`lefthook.yml`](lefthook.yml)) runs a fast read-only subset and never rewrites or stages files. Fix findings with `bun run fmt` or `bun run contracts`.

## Changes

- Keep a change to one concern; each commit should pass `bun run check`.
- Commit subjects follow lightweight [Conventional Commits](https://www.conventionalcommits.org/) (`feat(core): …`, `fix(api): …`, `docs: …`).
- Behaviour changes need tests that exercise the real path: owned fixture processes, the real HTTP server, a real browser. Tests must never signal a process they did not start.
- Fixture processes come from `stackscan-fixtures` (`crates/fixtures`): `Fixture::spawn` launches the `stackscan-fixture` binary, waits for its ready line, records PID, start time, cwd and endpoints, and cleans up through its own child handle after an identity check. A fixture also exits on stdin EOF (its owner died) and after `--max-lifetime-secs`. `stackscan_fixtures::lsof` gives independent ground truth. `FixtureSpec::serve` picks what connections get (a non-HTTP banner by default, or HTTP 200 with a title, 404, 500, hang, huge body, a redirect to its own `/next`, self-signed HTTPS), `::record` keeps what each connection sent, and `::args` sets an ignored command line (fake secrets, markup). Only processes in a project are probed automatically, so probe fixtures run in a project directory (`FixtureSpec::cwd`). Credential-shaped test values are assembled at run time so no secret-scanner-flagged literal enters the source. Run fixture tests with `cargo nextest run --workspace <filter>`: with `-p` alone they fail on a clean target, or run a stale fixture binary on a warm one.
- If a change alters a contract in `blueprint/`, update the smallest relevant section in the same change and say why.
- Security-sensitive areas (HTTP guards, redaction, process signalling) get a test for the failure case, not only the happy path.

Security issues: see [SECURITY.md](SECURITY.md); please do not open public issues for them.

## Running the binary

`target/release/stackscan serve [--port <n>] [--no-open]` serves the embedded UI and API on `127.0.0.1` (default port 47831) until SIGINT/SIGTERM. An occupied port exits 1 with a clear message; usage errors exit 2. Debug builds also accept hidden flags that release builds do not have: `--dev`, which `bun run dev` passes, admits the Vite authority `127.0.0.1:47833`; `--probe-only=<PID,…>` restricts automatic probing to those PIDs (empty: none), for tests.

API (every route, the embedded UI included, refuses a `Host` other than `127.0.0.1:<port>` or `localhost:<port>`; API routes refuse requests a browser marks cross-origin):

| Route               | Behavior                                                                                                                                                                                                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GET /api/health`   | `Health`: name and version.                                                                                                                                                                                                                                                                |
| `GET /api/session`  | `Session`: the per-instance action token (memory only, new on every start).                                                                                                                                                                                                                |
| `GET /api/snapshot` | The latest `Snapshot` of the single scanner.                                                                                                                                                                                                                                               |
| `GET /api/events`   | Server-Sent Events: a `snapshot` event with the current snapshot, then one per publication (newest only for slow readers; no replay). Ends when the server stops.                                                                                                                          |
| `POST /api/refresh` | `{}` → scans now and returns the `Snapshot`; at most one per second (`429` + `Retry-After`). Needs the same-origin `Origin`, `Content-Type: application/json` and `X-Stackscan-Token: <token>`.                                                                                            |
| `POST /api/probe`   | `ProbeRequest` (`{identity, port}`) → probes that endpoint now ("try HTTP") and returns its `HttpProbe`, marked `requested`. Same requirements and limit as refresh; `409 staleIdentity` if the identity is not current, `422 notProbeable` for another user's or a non-loopback endpoint. |

Refusals are `ApiError` JSON (`hostNotAllowed` 421, `originNotAllowed`/`tokenRequired` 403, `unsupportedMediaType` 415, `payloadTooLarge` 413, `invalidPayload` 400, `staleIdentity` 409, `notProbeable` 422, `rateLimited` 429, `scannerBusy`/`scannerStopped` 503). A body over 16 KiB is `413` both when `Content-Length` declares it (refused before reading) and when a chunked body exceeds it while being read; a body within the limit that is not the expected JSON is `400`. Every response carries a strict CSP (`default-src 'self'`, no inline script), `nosniff`, `no-referrer` and `frame-ancestors 'none'`.

`serve` probes HTTP candidates on loopback (the current user's processes that run in a project) with one bounded `GET /` per process and port every 15 s (see `crates/app/src/prober.rs`); results appear on each endpoint's `http` from the next scan on.

`stackscan list` scans once and prints one row per listening port (PID, process, port, project and package, branch, bind addresses), with warnings on stderr. Command lines appear only in `--json`, redacted. `stackscan list --json` prints the same scan as the `Snapshot` contract (`web/src/generated/contracts.ts`): revision 1, the observation (services, projects, warnings) and the scanner status; it never probes, so HTTP candidates read `pending`. A one-shot scan sees each process once, so CPU reads `warmingUp`; `serve` samples it from the second scan on. Both exit 0 with partial data (warnings) and 1 if sockets cannot be enumerated at all. `list` always runs its own one-shot scan; attaching to a running instance's `/api/snapshot` arrives with S08.

Project attribution runs `git` from the active developer directory (`xcode-select -p`); without Xcode or the Command Line Tools, services inside Git worktrees are reported with an `attributionUnavailable` warning instead of a project.

`serve` runs the instance's single scanner loop (3 s cadence) on its own thread and logs scan failures, recoveries and overruns to stderr; nothing per request is logged.

The rest of the [command contract](blueprint/architecture.md#configuration-and-packaging) arrives with later slices: tray mode with S08; `doctor`, `startup` and the release/install commands with S09.
