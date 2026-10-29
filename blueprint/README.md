# Kossé blueprint

## Intent

Ship a small tool a developer wants to keep running, not a general-purpose process manager. It observes services started by terminals and agents; it does not take ownership of their startup.

| Document | Responsibility |
|---|---|
| [Product](product.md) | Scope, interactions, acceptance |
| [Architecture](architecture.md) | Stack, service model, security, command contract |
| [Project metadata](project-metadata.md) | Optional integration without mandatory orchestration |
| [Implementation plan](implementation-plan.md) | Work packages and dependency graph; live status stays in the tracker |
| [Verification](verification.md) | Fixtures, entry-point proof, completion gates |
| [Loop](loop.md) | How a fresh agent resumes and executes bounded work |
| [References](references.md) | Preferences, library candidates, provenance |

This borrows only the general idea of a blueprint workflow: explicit intent, ordered work, testable acceptance, resumable context. It does not duplicate another project's skill adapters or force an approval pause after every diff. A user-invoked loop can finish bounded local work without repeated confirmation, subject to the stop conditions in loop.md.

## Definition of complete

A locally packaged macOS arm64 application provides tray and dashboard views (the dashboard also on demand via `kosse open`, without a resident app), identifies real fixture services, samples metrics, safely stops fixture processes, installs/uninstalls in a sandbox, and passes the full command suite. Source checkout instructions and local release instructions work. OS-only/human verification gaps are blockers or explicitly user-accepted deferrals, not silent passes.

Not required for v1: public hosting, signed/notarized distribution, Homebrew tap, Windows/Linux parity, cloud telemetry, public release. No external credentials should be necessary to complete the local artifact.

## Status

v0.1.0 was delivered as a local artifact (2026-10-28, slices S01–S10, installed-entrypoint audit in [S10](../docs/evidence/S10.md)). S11 drove and fixed the real window; S12 replaced the browser dashboard and WKWebView window with a native Rune window, released as v0.2.0 through GitHub Releases. Evidence per slice is indexed in [`docs/evidence/`](../docs/evidence/README.md). Human checks remain open and are not counted as passed: the native tray/window checklist (S08, S12), VoiceOver and a real login cycle (S09).

Next phase, in this order unless the owner reorders it: the OpenTUI terminal client (`kosse tui`, S07t in the plan); persisted extras the product contract names but v1 does not implement (hidden identities and rules); the opt-in integrations of the [extended metadata level](project-metadata.md#three-levels); signing, notarization and a published tap or download, which stay approval-dependent.

## Decisions

These were planning decisions; S12 changed the dashboard surface, and the rest hold.

- macOS arm64 is the required v1 target; other platforms get explicit unsupported behavior, not claimed support.
- Native tray menu plus a native dashboard window are required. Since S12 the dashboard is a Rune window (`kosse window` child process) reading the guarded loopback API; there is no browser dashboard (`/` is an information page). A custom graphical tray popover is deferred.
- Rust process/service core and Rust UI. Kossé is a resident tool that must stay nimble: no web view or JavaScript runtime; binary size and owner/window CPU and memory are reported separately (the S01b–S11 50 kB web budget is retired).
- Bind to IPv4 loopback, default port 47831; an occupied port must fail clearly rather than silently move a bookmarked dashboard URL.
- One resident application can own scanner, API, tray. Headless serve is also supported, but must not run a second instance on the same endpoint.
- Small versioned JSON config; no database/history store needed.
- Independent implementation, not a PortDoc fork. Product code is MIT-licensed (see LICENSE); dependency license audit is required.
