<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/kosse-lockup-night.svg">
    <img src="brand/kosse-lockup.svg" alt="Kossé" height="56">
  </picture>
</h1>

A native macOS tray menu and a lightweight local dashboard for the development services you already run: **discover, identify, open, inspect, stop.**

You start dev servers from terminals, scripts and coding agents. A day later there are a dozen listeners on unremembered ports, some from worktrees you forgot. Kossé shows them grouped by repository, worktree and package, with honest observations (listening, reachable over HTTP, or neither) and current CPU and memory. It stops a forgotten process only after confirming its exact identity. It never asks you to register projects or change how you launch them.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/dashboard-dark.png">
  <img src="docs/screenshots/dashboard-light.png" alt="The native Kossé dashboard in Rune, filtered to two storefront worktrees, with listening ports, HTTP observations, process metrics and Inspect and Stop actions." width="900">
</picture>

<sub>Owned fixture processes in the native Rune window.</sub>

> **Status: v0.2.0, macOS on Apple silicon, unsigned** ([GitHub Releases](https://github.com/davidpp/kosse/releases)). The native Rune window is verified through the real window and the packaged CLI/API ([S12 evidence](docs/evidence/S12.md)). VoiceOver, physical menu-bar interaction and a real login cycle remain unverified.

## What it does

- **Discovers** every TCP listener on the Mac and the process behind it (PID, start time, user, redacted command line, working directory, CPU, memory), checked against `lsof`.
- **Identifies** the repository, worktree, branch and package each one runs in, from the process's working directory. Two checkouts of the same repository stay separate.
- **Observes** loopback HTTP endpoints of your own project processes with one bounded `GET` every 15 s, and reports what it saw (200, 404, a TLS failure, not HTTP) instead of a health verdict. Open appears only for endpoints that answered HTTP.
- **Stops** one process safely: SIGTERM after re-checking its exact identity and confirming every port it holds, then reports whether it exited, is still running (force is then offered, with a second confirmation), was respawned by its parent, or someone else took the port.
- **Three views on one scanner:** a native menu-bar menu, a native Rune dashboard, and `kosse list` in the terminal.

## What it doesn't do

- Start, restart, wrap or supervise anything; allocate ports; proxy or provision certificates.
- Kill process groups or parents, escalate on its own, or clean up "stale" services automatically.
- Write into your repositories, phone home, check for updates, or load remote assets.
- Map container-internal services to repositories, keep metric history, or run on Linux or Windows (v1 is macOS arm64 only).

## Install

Kossé is one self-contained executable with the dashboard built in; it needs no Bun, Node or other runtime. It is **not signed or notarized** (Apple silicon's mandatory signature is the linker's ad-hoc one).

**From GitHub Releases** (macOS on Apple silicon):

```sh
curl -fsSL https://github.com/davidpp/kosse/releases/latest/download/install.sh | sh
```

The script downloads the latest archive and its `.sha256`, refuses it unless the SHA-256 matches, verifies its build provenance with `gh attestation verify` when an authenticated [GitHub CLI](https://cli.github.com) is installed, and installs `~/.local/bin/kosse`. It changes nothing else. Options: `curl -fsSL …/install.sh | sh -s -- --version X.Y.Z --prefix <dir>` (installs `<dir>/bin/kosse`).

By hand, from the [release page](https://github.com/davidpp/kosse/releases/latest):

```sh
v=X.Y.Z; a=kosse-$v-aarch64-apple-darwin   # the release version
curl -fLO https://github.com/davidpp/kosse/releases/download/v$v/$a.tar.gz
curl -fLO https://github.com/davidpp/kosse/releases/download/v$v/$a.tar.gz.sha256
shasum -a 256 -c $a.tar.gz.sha256
gh attestation verify $a.tar.gz -R davidpp/kosse   # optional: built by this repository's release workflow
tar -xzf $a.tar.gz
mkdir -p ~/.local/bin && install -m 0755 $a/kosse ~/.local/bin/
```

Files downloaded with `curl` carry no quarantine attribute, so the unsigned binary runs as is. A browser download is quarantined and macOS refuses to open it; only for an archive whose origin and checksum you trust, `xattr -d com.apple.quarantine kosse` lifts Gatekeeper's check for that file.

If `~/.local/bin` is not on your `PATH` (it is not on a stock Mac): `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile`, then open a new terminal. Then run `kosse` to start the menu-bar app, and optionally `kosse startup enable` to start it at each login. `kosse doctor` checks the setup.

**Homebrew.** Each release also carries `kosse.rb`, a formula for exactly that archive. There is no public tap yet; to use it, `brew tap-new <you>/kosse`, copy `kosse.rb` into that tap's `Formula/`, then `brew install <you>/kosse/kosse`.

**Local release from source** (with the [development setup](#development)):

```sh
bun run release:local    # artifacts/kosse-<version>-aarch64-apple-darwin.tar.gz + .sha256 + notices + formula
bun run install:local    # verifies it, installs ~/.local/bin/kosse (--prefix <dir> elsewhere)
bun run uninstall:local  # removes exactly what it installed; keeps your configuration unless --purge
```

## Usage

| Command                                 | What it does                                                                                                                                                                                |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `kosse`                                 | The menu-bar app: tray menu plus the native Rune dashboard, opening its window (`--no-open` to skip). A second `kosse` brings the running app's window forward instead of starting another. |
| `kosse open`                            | The dashboard window without anything resident: serves until you close the window, then exits. With the menu-bar app running it shows that app's window.                                    |
| `kosse serve`                           | Foreground owner without a tray: opens the native window (`--no-open` to skip); `kosse open` shows that window. `/` is only an information page, not a browser dashboard.                   |
| `kosse list [--json]`                   | One row per listening port with project, branch and package. Uses the running instance's snapshot when there is one, otherwise scans once. `--json` prints the `Snapshot` contract.         |
| `kosse doctor [--json]`                 | Checks platform, binary and `PATH`, configuration, who holds the port, `git`, login startup. Changes nothing.                                                                               |
| `kosse startup enable\|disable\|status` | Opt-in login startup: a per-user LaunchAgent running this `kosse --no-open` at each login (no `KeepAlive`, so Quit stays quit). Off until enabled.                                          |
| `kosse config schema\|path\|move`       | The `kosse.json` JSON Schema, the configuration file's location, and re-attaching a profile after moving a project.                                                                         |

**Tray menu:** projects with their branch; each service as `name :port` with Open (only if it answered HTTP), Inspect and Stop… (both open the dashboard window at that service; Stop… is the dashboard's own confirmation); Open Dashboard; Quit Kossé. Quit stops Kossé only, never the services it shows. The icon switches to its attention variant only when discovery fails or stalls.

Every flag and route is documented in [CONTRIBUTING.md](CONTRIBUTING.md#running-the-binary). Optional project names and links live in a central profile ("Customize this project" in the dashboard) or a read-only `kosse.json` in the repository ([schema](docs/kosse.schema.json), [format](blueprint/project-metadata.md)).

## Architecture

```text
            ┌──────────── kosse (one process per port) ────────────┐
 tray menu ─┤  scanner thread ──► snapshot ──► Axum API + SSE      ├── dashboard (Rune child
 (main      │  (sockets, sysinfo,   ▲          (127.0.0.1 only,    │    `kosse window`)
  thread)   │   git, profiles)      │           Host/Origin/token) │
            │  bounded loopback HTTP probes                        ├── kosse list / doctor (CLI)
            └──────────────────────────────────────────────────────┘
crates/core  discovery · identity · attribution · action policy · profiles (no UI or server deps)
crates/app   CLI · scheduler · prober · API · tray · window
crates/app/src/native/  Rune window, controls, profile editor and guarded API client
```

Every view reads the same snapshot and uses the same action policy; there is no second scanner or alternate destructive path. The native UI consumes the Rust contracts directly; generated TypeScript supports the development harness, and CI fails on drift. Details: [architecture](blueprint/architecture.md), [product contract](blueprint/product.md), [verification gates](blueprint/verification.md).

The native port has no web bundle or WebKit process. Always on, the menu-bar app idles at about 0.3 % CPU and 12 MB with the window closed; the window is a child process that exits when closed and costs 65–280 MB while open, depending on use ([footprint](docs/evidence/S12.md#footprint)). The release executable is about 21 MB.

## Security model

Kossé binds to loopback only and treats every website, repository file and process-derived string as untrusted: `Host` is checked on every route, cross-origin API requests are refused, mutations need the same-origin `Origin`, JSON, a bounded body and a per-instance token, there are no CORS headers, and a strict CSP applies to every response. Stop requires a fresh identity and explicit confirmation; other users' processes and Kossé itself are never signalled or probed. Command lines are redacted before they leave the core. The threat model and its limits (heuristic redaction, the PID-reuse window, same-user malware) are in [SECURITY.md](SECURITY.md).

## Development

Requires macOS on Apple silicon, the Xcode Command Line Tools and [mise](https://mise.jdx.dev); every tool version is pinned in the repository.

```sh
mise trust && mise install
bun install --frozen-lockfile
bun run check      # the full non-mutating gate, identical to CI
bun run dev        # build and run the native menu-bar app
bun run app -- start   # the real menu-bar app + window on fixtures (docs/agent-drive.md)
bun run test:window    # real Rune window suite (transparent, click-through window)
bun run build && target/release/kosse
```

Tests run against real things: owned fixture processes checked against `lsof`, the real HTTP server, the actual Rune window, the packaged archive in a sandbox. They never signal, probe or reconfigure anything they did not start. See [CONTRIBUTING.md](CONTRIBUTING.md) for every command and convention, and [AGENTS.md](AGENTS.md) for the rules coding agents follow here.

## Limitations

- macOS on Apple silicon only; not signed or notarized; no public Homebrew tap.
- Redaction is heuristic: a bare positional secret without a recognisable key or shape is shown.
- Other users' sockets are invisible without administrator rights (reported as a snapshot warning); Kossé never asks for them.
- CPU needs two observations: a fresh process reads "warming up" for one scan, and `kosse list` (one shot) always does.
- macOS 14+ activation is cooperative: a window brought forward from a terminal command may stay behind the terminal.
- Without the Xcode Command Line Tools, Git worktrees are not attributed (reported, never guessed).
- No metric history, container attribution or terminal UI yet; the OpenTUI client (`kosse tui`) is the next phase.

## How this project is built

Kossé is developed blueprint-first. [`blueprint/`](blueprint/README.md) holds the product contract, architecture, verification gates and a dependency-ordered [slice plan](blueprint/implementation-plan.md). Each slice was implemented and verified through real entrypoints, recorded in [`docs/evidence/`](docs/evidence/) (commands, results, measurements and what stayed unverified), then reviewed by independent lenses (correctness and security, documentation and product) before it landed. Mocked tests alone never count as done, and human-only checks are never marked passed by an agent. The release history is in [CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE). Kossé is an independently written project; see [provenance notes](blueprint/references.md#inspiration-and-boundaries). Third-party licenses ship in the archive's `THIRD_PARTY_NOTICES.md`.

## Brand

_Kossé ?_ is Québécois for "what's that?", the question you ask about an unfamiliar port. The **Relay** mark is a separated K, with dedicated optical geometry for the menu bar. Québec-inspired blue and a Kibernetyk Mono wordmark carry the new identity. The identity, palette and tokens are in [`brand/BRAND.md`](brand/BRAND.md); the assets and how to regenerate them are in [`brand/`](brand/README.md).
