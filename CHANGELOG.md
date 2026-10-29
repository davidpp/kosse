# Changelog

All notable changes to Kossé are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- Homebrew: `brew install davidpp/tap/kosse`. The release workflow commits each release's formula to [davidpp/homebrew-tap](https://github.com/davidpp/homebrew-tap), so `brew upgrade` follows new versions.
- Agent read surface ([S13](docs/evidence/S13.md)): `kosse list` prints Markdown tables (columns `PROJECT`, `BRANCH`, `SERVICE`, `PORT`, `HTTP`, `PROCESS`, `PID.START`, `AGE`) and takes `--here`, `--project`, `--port` and `--service` (also for `--json`); `kosse url <service|port>` prints a service's local URL; `kosse port <n>` says whether a port is free and who holds it. Documented exit codes (0/1/2/3/4) and `CommandError` JSON on stderr with `--json`.
- `kosse wait (--port <n> | <service>) [--http | --gone] [--timeout <dur>]` ([S14](docs/evidence/S14.md)): waits until a service listens, answers HTTP or is gone, following the running instance's event stream or scanning every second, with requested probes under the existing probe policy; prints the last observation, exits 0 when met and 1 on timeout (`WaitAnswer` with `--json`).
- Agent profile authoring ([S15](docs/evidence/S15.md)): `kosse profile context|show|set|remove|check|prompt` gives the coding agent in a repository what Kossé observes there, validates and previews (`set --dry-run`, computed by the scanner's own matching) a central profile, and writes only Kossé's configuration. Stored profiles carry a revision: `--expect <revision>` and `POST /api/profile`'s `expectedRevision` refuse a lost update (`revisionConflict`).

### Changed

- `kosse list`'s default output is Markdown instead of space-aligned columns, without the bind-address column (in `--json`). The running instance to ask is now `--instance-port` (was `--port`, which now filters by service port).

## 0.1.1 - 2026-10-29

First published release: a GitHub Release with checksum and build provenance (0.1.0 was built and verified locally only, never tagged).

### Changed

- New Relay brand identity: a separated K mark with dedicated menu-bar templates, a Québec-inspired blue palette in light and dark, a Kibernetyk Mono wordmark drawn as SVG outlines (no web font), a new app icon and favicon, and split-stem preview icons for development builds.

### Fixed

- Dialogs in the dashboard window (Stop, Force, Customize, the removal confirmation) drew as a 2px sliver behind the backdrop in macOS's WKWebView; service rows overflowed the window horizontally between 768 and about 820 px wide ([S11](docs/evidence/S11.md)).
- Kossé's own row offers no Stop (dashboard and tray); a deep link to it explains why instead of a confirmation that would be refused.
- The dashboard window's Dock and ⌘-Tab icon is Kossé's, not the generic executable icon.

### Added

- `bun run app` drives the real app and window from an agent (debug-only control socket; [docs/agent-drive.md](docs/agent-drive.md)); `bun run test:window` is the real-window regression suite. Debug builds carry a preview identity (split-stem preview icons, `Kossé · preview (<branch>)`).
- Tag-driven GitHub releases (`.github/workflows/release.yml`): a `vX.Y.Z` tag is checked against the Cargo version and this changelog, built from a clean slate through `check` and `test:package`, attested for build provenance and published with the archive, its SHA-256, the Homebrew formula and `install.sh`, a `curl | sh` installer that verifies the checksum (and provenance when `gh` is available) ([CONTRIBUTING](CONTRIBUTING.md#releasing)).

## 0.1.0 - 2026-10-28

First version: built, packaged and verified locally on macOS arm64; not signed, notarized, tagged or published. Each item links the slice evidence in [`docs/evidence/`](docs/evidence/).

### Added

- Rust workspace (`crates/core`, `crates/app`), embedded web UI and one non-mutating gate, `bun run check`, shared by the pre-commit hook and CI ([S01](docs/evidence/S01.md)).
- Dashboard runtime on TanStack Redact with TanStack Query, within a 50 kB gzipped JS + CSS budget enforced by `check` ([S01b](docs/evidence/S01b.md)).
- TCP listener discovery and process inspection (PID + start time, user, redacted command line, cwd), checked against `lsof` with an owned fixture-process harness ([S02](docs/evidence/S02.md)).
- Project attribution (repository, linked worktree, branch or detached HEAD, nearest package) and CPU/memory sampling on a single 3 s scanner; `kosse list [--json]` ([S03](docs/evidence/S03.md)).
- Loopback-only HTTP API with Server-Sent Events, Host/Origin/token guards and CSP; bounded HTTP probes with offline TLS verification ([S04](docs/evidence/S04.md)).
- Identity-checked stop (`SIGTERM`) with an explicit, expiring force (`SIGKILL`) offer and honest outcomes: exited, still running, respawned, endpoint taken ([S05](docs/evidence/S05.md)).
- Central project profiles and optional read-only repository `kosse.json`, with a JSON Schema and `kosse config schema|path|move` ([S06](docs/evidence/S06.md)).
- Project-card dashboard: search, All listeners, Inspect sheet, Stop and Force confirmations, deep links, live/stale/disconnected states, light and dark ([S07](docs/evidence/S07.md)).
- Native menu-bar app, a single dashboard window (WKWebView child process), `kosse open` without a resident app, and single-instance reuse verified by socket owner and API ([S08](docs/evidence/S08.md)).
- Local release archive with checksums, third-party notices and manifest; prefix-aware `install:local`/`uninstall:local`; opt-in login startup (`kosse startup`); `kosse doctor` ([S09](docs/evidence/S09.md)).
- The dashboard's All listeners choice is saved in Kossé's configuration (`/api/preferences`) ([preferences](docs/evidence/preferences.md)).
- Installed-binary acceptance audit, operator documentation and threat model ([S10](docs/evidence/S10.md)).

### Known limitations

- The native tray/window checklist and a real login cycle await a person at a desktop.
- macOS on Apple silicon only; unsigned; no tap or public download.
