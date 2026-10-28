# Rename evidence: Stackscan → Kossé

Recorded 2026-10-28 on macOS 26.6.2, arm64, Rust 1.99.1, Bun 1.4.2, as an uncommitted tree on top of `e7419c6`. The plan slice IDs are unchanged.

## What changed

- Display name **Kossé**: page `<title>`, dashboard heading, connection copy, user-facing error messages, docs prose. The README lockup, tagline and `## Brand` section from the branding work are kept as they were; `brand/` is untouched.
- Machine-facing names are ASCII `kosse`: crates `kosse-core`, `kosse` (binary `kosse`), `kosse-fixtures` with the `kosse-fixture` binary (`crates/fixtures/src/bin/kosse-fixture.rs`); module paths `kosse_core`/`kosse_fixtures`; npm workspaces `kosse-workspace` and `@kosse/web`; `KOSSE_UPDATE_CONTRACTS`; the `X-Kosse-Token` header; health `name` (`kosse`), CLI and log prefixes, the `kosse-scanner` thread, the probe `User-Agent` (`kosse/<version> (probe)`), the fixture banner `kosse-fixture`; CI, Lefthook, scripts and issue templates.
- `blueprint/architecture.md` records the macOS bundle identifier `com.davidpp.kosse` for when an .app is packaged.
- Earlier evidence files (S01–S04, S01b) carry a note that their runs predate the rename; their commands, paths and quoted output use the new names so they stay runnable.
- `Cargo.lock` was regenerated with `cargo update --workspace` (only the three workspace entries changed). In `bun.lock` the three workspace-name lines were renamed in place: `bun install` refuses to re-resolve because `@tanstack/redact@0.2.1` is still inside the repository's `minimumReleaseAge` window, and lowering that guard was not done. `bun install --frozen-lockfile` from a clean tree then accepts the lockfile, which is the check that it matches `package.json`.
- No compatibility aliases or redirects: there are no released users or persisted data.

## Commands and results

| Command                                                                                                  | Result                                                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bun run contracts`                                                                                      | `kosse-core::contracts` passes; the regenerated `web/src/generated/contracts.ts` is byte-identical to the renamed file.                                                       |
| `rm -rf node_modules web/node_modules web/dist target && bun install --frozen-lockfile && bun run check` | Exit 0. Vitest 7/7, bundle 31.29 kB of 50 kB gzipped, nextest 124/124, release build.                                                                                         |
| `target/release/kosse --version`                                                                         | `kosse 0.1.0`.                                                                                                                                                                |
| `target/release/kosse list --json`                                                                       | Real scan: revision 1, 20 services, 4 warnings.                                                                                                                               |
| `target/release/kosse serve --no-open --port <free>`, then SIGTERM to its PID                            | `/` has `<title>Kossé</title>`; `/api/health` → `{"name":"kosse","version":"0.1.0"}`; stderr `kosse 0.1.0 listening on …` then `kosse stopped`, exit 0; port free afterwards. |
| `bun run dev`, curl and headless Chrome via `127.0.0.1:47833`, then SIGTERM to the runner                | Health `kosse` through the Vite proxy; the page title is `Kossé` and it renders "Connected to kosse"; backend and Vite exit, 47832/47833 free, no started process left.       |

## Remaining `stackscan` references

`rg -i stackscan` outside `target`, `node_modules`, `.git`, `vendor`, `.claude` and `brand`:

- `github.com/davidpp/stackscan` in `Cargo.toml`, `package.json`, `SECURITY.md`, `.github/ISSUE_TEMPLATE/config.yml` and `blueprint/references.md`: the repository URL stays until the owner decides on the remote.
- `vendor/listeners/KOSSE-PATCH.md`, named in `Cargo.toml` and `docs/evidence/S02.md`: a file in the vendored third-party tree, which is not renamed.
- The rename notes in the earlier evidence files and this file: historical.
