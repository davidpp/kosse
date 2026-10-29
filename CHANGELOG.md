# Changelog

All notable changes to Kossé are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## 0.2.0 - 2026-10-29

The dashboard is now a native window compiled into `kosse`; nothing in Kossé runs a web view or a JavaScript runtime. The first version published as a GitHub Release. Evidence: [S12](https://github.com/davidpp/kosse/blob/v0.2.0/docs/evidence/S12.md).

### Changed

- The dashboard is a native [Rune](https://github.com/anomalyco/rune) window (`kosse window`, a child of its owner) compiled into the `kosse` executable. It reads the same guarded loopback API and SSE stream as before and uses the same action policy; the tray, `serve` and `kosse open` each reuse their owner's one window.
- `/` on the loopback port is now a script-free information page; the API is unchanged. `kosse serve` opens the native window unless `--no-open`, and `kosse open` against a running `serve` shows that owner's window instead of opening a browser.
- A window that cannot start is reported on stderr and in the tray; nothing falls back to the browser.
- Customize this project is a structured editor: a project/service sidebar, explicit fields for names, matching rules, protocol, probe path, links and availability, repository values shown as inherited and read-only, and a fixed Save/Cancel footer.
- New identity: the Relay K mark with dedicated menu-bar geometry, Québec-inspired blue and a Kibernetyk Mono wordmark. The window uses bundled Inter for interface text. Debug builds carry a split-stem preview identity (icons and `Kossé · preview (<branch>)`).

### Added

- `bun run app` starts and drives the real menu-bar app and native window on owned fixtures through a debug-only control socket ([docs/agent-drive.md](https://github.com/davidpp/kosse/blob/v0.2.0/docs/agent-drive.md)); `bun run test:window` is the real-window regression suite, driven with AppKit events.
- A tag-driven release workflow: a `vX.Y.Z` tag runs the full gate and the packaged-archive test, attests build provenance and publishes a GitHub Release with the archive, its SHA-256 file, a Homebrew formula and an `install.sh` that verifies the checksum (and the attestation when `gh` is available).

### Removed

- The browser dashboard: WebKit/wry, the Vite bundle and its 50 kB budget, TanStack Redact and Query, Vitest and the Playwright suite (`bun run test:e2e`, `bun run screenshots`, `bun run budget`).
- The debug-only `--dev` flag of `kosse serve` and the Vite development server; `bun run dev` builds and runs the native app.

### Fixed

- Kossé's own row offers no Stop (dashboard and tray); a deep link to it explains why instead of a confirmation that would be refused.
- The dashboard window's Dock and ⌘-Tab icon is Kossé's, not the generic executable icon.
- The 0.1.0 WKWebView window drew dialogs as a 2px sliver and let service rows overflow horizontally between 768 and about 820 px ([S11](https://github.com/davidpp/kosse/blob/v0.2.0/docs/evidence/S11.md)); fixed there, and superseded by the native window.

### Known limitations

- VoiceOver and the OS accessibility tree, physical pointer feel, menu-bar and Dock appearance under real interaction, activation alongside other apps and a real login cycle are not verified.
- macOS on Apple silicon only; not signed or notarized; no public Homebrew tap.

## 0.1.0 - 2026-10-28

First version: built, packaged and verified locally on macOS arm64; not signed, notarized, tagged or published. Its browser dashboard and WKWebView window were replaced in 0.2.0. Each item links the slice evidence in [`docs/evidence/`](docs/evidence/).

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

[Unreleased]: https://github.com/davidpp/kosse/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/davidpp/kosse/releases/tag/v0.2.0
