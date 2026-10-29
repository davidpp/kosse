# Contributing

Kossé ships one Rust executable: a macOS menu-bar owner, a native Rune window and the CLI. Rune is pinned to a Git revision in Cargo.toml. There is no web build. Work is organised in slices defined in [`blueprint/implementation-plan.md`](blueprint/implementation-plan.md); each lands with tests through real entrypoints and an evidence note in [`docs/evidence/`](docs/evidence/README.md). AI-assisted contributions are welcome under the same bar: include the commands you ran and their results.

## Prerequisites

- macOS on Apple silicon (the only supported platform) with the Xcode Command Line Tools (`xcode-select --install`) for the linker and `git`.
- [mise](https://mise.jdx.dev), which pins every tool in [`mise.toml`](mise.toml): Rust (also in `rust-toolchain.toml`), Bun, cargo-nextest, cargo-deny, zizmor, oxipng, and cargo-about and cargo-auditable for `release:local`.
- Tests spawn short-lived owned listener processes on loopback (plus one on the `[::]` wildcard, skipped when the application firewall is on, so it never triggers a prompt) and call `/usr/sbin/lsof` as ground truth. The default-port test binds `127.0.0.1:47831`, or only connects to it when something already listens there.
- A Kossé you run yourself may keep running during `bun run check`: every probe carries its sender's instance ID, and probe tests count only their own server's requests.
- `bun run test:window` and `bun run app` need an unlocked graphical session.

```sh
mise trust && mise install      # once per machine
bun install --frozen-lockfile   # also installs the repo-local Lefthook hook
bun run check
```

Installing needs network access (mise downloads, `bun install`, `cargo fetch`, and the advisory database for `bun run audit`). Bun runs development and verification scripts; the installed app needs no runtime.

## Commands

| Command                                     | Purpose                                                                                |
| ------------------------------------------- | -------------------------------------------------------------------------------------- |
| `bun run dev`                               | Build and run the native app on this Mac                                               |
| `bun run build`                             | `cargo build --release`: `target/release/kosse`                                        |
| `bun run app -- start`                      | Open a preview on owned fixtures and isolated configuration                            |
| `bun run app -- stop`                       | Close that preview and release its fixtures                                            |
| `bun run check`                             | Non-mutating formatting, lint, types, Rust tests, contract freshness and release build |
| `bun run test:window`                       | Real Rune/AppKit interaction suite; unlocked screen required                           |
| `bun run contracts`                         | Regenerate `contracts/contracts.ts` and the profile JSON schema                        |
| `bun run fmt`                               | Apply Rust and repository formatting                                                   |
| `bun run audit`                             | Dependency/license/source and workflow audit                                           |
| `bun run release:local`                     | Build an archive, checksum, notices and formula                                        |
| `bun run test:package`                      | Install, exercise, upgrade and uninstall the archive in an isolated sandbox            |
| `bun run install:local -- --prefix <dir>`   | Install only recorded archive files at an explicit prefix                              |
| `bun run uninstall:local -- --prefix <dir>` | Remove only the recorded installation                                                  |

CI runs `check`, then `test:window` and `test:package`, and fails if `check` leaves the tree dirty; `audit` is a separate job. The pre-commit hook ([`lefthook.yml`](lefthook.yml)) runs a fast read-only subset and never rewrites or stages files; fix findings with `bun run fmt` or `bun run contracts`.

## Changes

- Keep a change to one concern; each commit should pass `bun run check`. Commit subjects follow lightweight [Conventional Commits](https://www.conventionalcommits.org/) (`feat(core): …`, `fix(api): …`, `docs: …`).
- Behaviour changes need tests that exercise the real path: owned fixture processes, the real HTTP server, the real native window. A UI change is seen in the real window (`bun run app`, [docs/agent-drive.md](docs/agent-drive.md)) and covered by `test:window`.
- `cargo nextest run --workspace <filter>` builds the fixture executable as well as the selected tests (with `-p` alone, fixture tests fail on a clean target or run a stale fixture binary). Fixtures come from `kosse-fixtures` (`crates/fixtures`): `Fixture::spawn` records PID, start time, cwd and endpoints and cleans up through its own child handle after an identity check; `FixtureSpec` chooses what connections get (non-HTTP banner, HTTP statuses, hang, huge body, redirect, self-signed HTTPS), an ignored command line, `SIGTERM` handling, a pinned port, or a supervisor that respawns its child. `kosse_fixtures::lsof` is independent ground truth. Credential-shaped test values are assembled at run time.
- Tests never stop or probe a process they did not start, never open a window or browser on the desktop, and never touch the real configuration, launch agents or installation prefix (the hooks are under [Running the binary](#running-the-binary)).
- Security-sensitive areas (HTTP guards, redaction, process signalling) get a test for the failure case, not only the happy path. If a change alters a contract in `blueprint/`, update the smallest relevant section in the same change and say why.
- Never commit screenshots containing the machine's real services or private paths.

Security issues: see [SECURITY.md](SECURITY.md); please do not open public issues for them.

## Running the binary

`kosse [--port <n>] [--no-open]` (no command) runs the menu-bar app: the single scanner, the guarded API on `127.0.0.1` (default port 47831) and the tray, opening the native window unless `--no-open`. `kosse serve [--port <n>] [--no-open]` runs the same owner in the foreground without a tray and also opens the window unless `--no-open`. `kosse open [--port <n>]` serves until its window closes, then exits; with a Kossé already on the port it shows that owner's window instead. `--port 0` picks a free port. An occupied port exits 1 with a clear message; usage errors exit 2. Quit, SIGINT and SIGTERM stop Kossé only, never observed services.

The window is the internal `kosse window <url>` child: Rune, connected only to its owner's loopback API after verifying socket ownership, fed routes over stdin and closed when its owner exits. A second launch reuses an instance only if the socket on the port belongs to the current user and it answers `GET /api/health` as `kosse` and `GET /api/snapshot` with an instance ID; there is no PID or lock file and never a second scanner or window. A `kosse open` session refuses a menu-bar app over it; anything else on the port is refused. If the window cannot start, stderr (and the tray) say why; no browser opens.

`kosse list [--json] [--port <n>]` prints the running instance's snapshot, or scans once (`--port 0` never asks); `--json` is the `Snapshot` contract (`contracts/contracts.ts`), with command lines redacted. `kosse doctor [--json] [--port <n>]` checks platform, executable and `PATH`, configuration, the port's occupant, `git`, `lsof` and login startup without changing anything. `kosse startup enable|disable|status` manages the per-user LaunchAgent `~/Library/LaunchAgents/com.davidpp.kosse.plist` (this `kosse --no-open`, `RunAtLoad`, `Aqua` only, no `KeepAlive`). `kosse config schema|path|move` prints the profile schema (`docs/kosse.schema.json`), the central file (`~/Library/Application Support/Kosse/config.json`) and re-attaches a moved project's profile.

API (every route refuses a `Host` other than `127.0.0.1:<port>` or `localhost:<port>`; API routes refuse cross-origin requests; unsafe methods need the same-origin `Origin`, `Content-Type: application/json`, a body of at most 16 KiB and `X-Kosse-Token` from `/api/session`; every response carries a strict CSP, `nosniff` and `no-referrer`; there are no CORS headers):

| Route               | Behavior                                                                                                                                  |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /`             | A script-free page saying Kossé is a native app; the dashboard is the window                                                              |
| `GET /api/health`   | `Health`: name, version and instance mode                                                                                                 |
| `GET /api/session`  | `Session`: the per-instance action token (memory only, new on every start)                                                                |
| `GET /api/snapshot` | The latest `Snapshot` of the single scanner                                                                                               |
| `GET /api/events`   | Server-Sent Events: the current snapshot, then one per publication (newest only for slow readers)                                         |
| `POST /api/refresh` | `{}` → scans now and returns the `Snapshot`; at most one per second (`429` + `Retry-After`)                                               |
| `POST /api/probe`   | `ProbeRequest` → probes one own-user loopback endpoint now ("try HTTP"); `409 staleIdentity`, `422 notProbeable`                          |
| `POST /api/stop`    | `ActionRequest` (`{instance, identity, ports}`, every confirmed port) → `SIGTERM` after a fresh identity check; returns an `ActionResult` |
| `POST /api/force`   | The same body → `SIGKILL`, only after a `stillRunning` stop of that identity in the last 60 s, else `409 forceNotOffered`                 |
| `POST /api/window`  | `WindowRequest` (`dashboard`, or `inspect`/`stop` with `pid` and `startedAtSecs`) → `204`: the owner shows its one window at that route   |
| `POST /api/profile` | `SaveProfileRequest` (`profile: null` removes it) → saves a central project profile; `404`, `422 invalidProfile`, `409 configUnavailable` |
| `/api/preferences`  | `GET` / `POST` `Preferences` (`{allListeners}`), stored in Kossé's configuration file                                                     |

Refusals are `ApiError` JSON with a typed `code`. Only `exited` is a successful stop; the other outcomes (`stillRunning`, `respawned`, `endpointTaken`, `observationFailed`, `stale`, `refused`, `permissionDenied`, `signalFailed`) are reported as such. The owner probes HTTP candidates (the current user's project processes on loopback, plus services the central profile declares `http`/`https`) with one bounded `GET` per process and port every 15 s; the scanner runs every 3 s and logs only failures, recoveries and overruns.

Environment variables. `KOSSE_CONFIG_DIR` replaces the configuration directory (every test sets it). Debug builds only, absent from release builds:

- `--probe-only=<PID,…>` restricts automatic probing to those PIDs (empty: none) and `--act-only=<PID,…>` restricts stop/force to those PIDs (matched with their start time) and their direct children, refusing PID 0 and 1. Every test owner runs with both.
- `KOSSE_TEST_WINDOW=headless|headless-once|transparent|fail`: the window is concealed (1% opacity, click-through, no Dock icon or activation) and prints `loaded <url>` and the open dialog per route; `headless-once` exits after the first load, `transparent` is what `test:window` drives, `fail` makes the window refuse to start.
- `KOSSE_TEST_OPENER=<program>` replaces `/usr/bin/open`, so service Open actions are captured instead of opening a browser.
- `KOSSE_TEST_TRAY=1` prints each rendered tray menu on stdout and takes `activate <item id>` lines on stdin. The status item itself is real and appears briefly.
- `KOSSE_TEST_LAUNCHCTL=<program>` replaces `/bin/launchctl`; startup tests also point `HOME` at a temporary directory.
- `KOSSE_WINDOW_CONTROL=1` with `KOSSE_WINDOW_CONTROL_SOCKET=<path>` (and `KOSSE_WINDOW_CONTROL_DIR` for screenshots) makes the window answer the agent control socket ([docs/agent-drive.md](docs/agent-drive.md)).

Debug builds also carry the preview identity (split-stem icons, window title `Kossé · preview (<branch>)`). Release builds contain neither it nor the control socket.

## Local release

`bun run release:local [--url <archive URL>]` writes, in `artifacts/` (ignored by Git):

- `kosse-<version>-aarch64-apple-darwin.tar.gz`: the release `kosse` (`cargo auditable build --release --locked`, so `cargo audit bin` can read its dependency list), `LICENSE`, `THIRD_PARTY_NOTICES.md`, `INSTALL.md` (from [`packaging/INSTALL.md`](packaging/INSTALL.md)) and `MANIFEST.json` (version, target, commit, whether the tree was dirty, `rustc` version, size and SHA-256 of each file).
- its `.sha256` file (`shasum -a 256 -c` format).
- `kosse.rb`, a Homebrew formula for exactly that archive from [`packaging/homebrew/kosse.rb.tmpl`](packaging/homebrew/kosse.rb.tmpl); its URL is a placeholder unless `--url` names where the archive is hosted.

The notices come from cargo-about with [`about.toml`](about.toml), which restates the [`deny.toml`](deny.toml) license policy (the script fails if the two differ), for the crates the release binary links on `aarch64-apple-darwin`, followed by the bundled Inter and Kibernetyk Mono font licenses.

`install:local` verifies the archive against its `.sha256` and `MANIFEST.json`, installs `<prefix>/bin/kosse` and `<prefix>/share/doc/kosse/`, and records what it installed; it refuses to replace a file it did not install or one changed since (`--force` for changed recorded files). `uninstall:local` removes exactly the unchanged recorded files and keeps the configuration unless `--purge`. `test:package` runs the installed release binary under `sandbox-exec`, which refuses outbound connections except to its own fixtures, so its automatic probes never reach real services. None of these publish, sign or enable login startup.

## Releasing

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`) and let Cargo update `Cargo.lock` (`cargo check --workspace`).
2. Move the `CHANGELOG.md` entries under `## [Unreleased]` into a new `## X.Y.Z - YYYY-MM-DD` section (that exact form: the workflow reads it), and update the compare links at the bottom.
3. Merge to `main` with CI green.
4. Tag and push: `git tag -a vX.Y.Z -m "Kossé X.Y.Z" && git push origin vX.Y.Z`.

[`.github/workflows/release.yml`](.github/workflows/release.yml) runs on the tag. It checks that the tag matches the Cargo version and that `CHANGELOG.md` has that section, runs `bun run check` and `bun run test:package`, builds the archive with `release:local --url` pointing at the release asset, checks that the archive's manifest names the tagged commit and a clean tree, attests build provenance, and publishes a GitHub Release whose notes are the changelog section, with these assets:

- `kosse-X.Y.Z-aarch64-apple-darwin.tar.gz` and its `.sha256`
- `kosse.rb`, the Homebrew formula for a personal tap
- `install.sh` ([`packaging/install.sh`](packaging/install.sh)), the installer behind `curl -fsSL https://github.com/davidpp/kosse/releases/latest/download/install.sh | sh`

Run the workflow manually (`workflow_dispatch`) for a dry run: it builds and checks the same files and uploads them as a workflow artifact without publishing anything. Signing, notarization and a public tap remain separate, approval-dependent work.
