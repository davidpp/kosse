# Contributing

Stackscan is built in vertical slices defined in [`blueprint/implementation-plan.md`](blueprint/implementation-plan.md). Each slice lands with tests through real entrypoints and a short evidence note in [`docs/evidence/`](docs/evidence/). AI-assisted contributions are welcome under the same bar: include the commands you ran and their results.

## Prerequisites

- macOS on Apple silicon (the only supported and tested v1 platform) with Xcode Command Line Tools (`xcode-select --install`) for the linker.
- Tests spawn short-lived owned listener processes on loopback, plus one on the `[::]` wildcard, and call `/usr/sbin/lsof` (ground truth) and `/usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate`. The wildcard test is skipped when the application firewall is on, so it never triggers an "accept incoming connections" prompt.
- Attribution tests build temporary Git repositories with `git` from the active developer directory (`$(xcode-select -p)/usr/bin/git`, the same binary Stackscan runs), ignoring global and system Git config and any inherited `GIT_*` variables.
- [mise](https://mise.jdx.dev), which pins every tool per repository in [`mise.toml`](mise.toml): Rust (also pinned in `rust-toolchain.toml`), Bun, Node (needed only by web test tooling), cargo-nextest, cargo-deny and zizmor.

```sh
mise trust && mise install   # once per machine
bun install --frozen-lockfile  # also installs the repo-local Lefthook hook
bun run check
```

Installing needs network access (mise tool downloads, `bun install`, `cargo fetch`, and the advisory database for `bun run audit`). The built `stackscan` binary embeds the web UI and needs neither Bun nor Node at runtime.

## Commands

| Command                     | Behavior                                                                                                                                                                                                                                                              |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bun run dev`               | Builds the debug backend and runs it on `127.0.0.1:47832` plus Vite on `127.0.0.1:47833`. Open `http://127.0.0.1:47833/`; Vite proxies `/api` to the backend (same origin, development only). Ctrl-C stops both; if either exits, the other is stopped.               |
| `bun run build`             | `vite build` into `web/dist`, then `cargo build --release`, embedding the bundle in `target/release/stackscan`.                                                                                                                                                       |
| `bun run check`             | Non-mutating full gate, identical to CI: `fmt:check`, `lint`, `typecheck`, `test`, release build.                                                                                                                                                                     |
| `bun run test`              | Vitest, web build, `cargo nextest run --workspace` (binary-level server tests; discovery, attribution, metrics, scheduler and `stackscan list` tests against owned fixture processes, temporary Git repositories and `lsof`; contract freshness), `cargo test --doc`. |
| `bun run lint`              | `lint:web` (`oxlint --deny-warnings`) and `cargo clippy --all-targets -- -D warnings`.                                                                                                                                                                                |
| `bun run typecheck`         | `tsc --noEmit` for `web/` and `scripts/`.                                                                                                                                                                                                                             |
| `bun run fmt` / `fmt:check` | `cargo fmt` + `oxfmt` (write / check).                                                                                                                                                                                                                                |
| `bun run contracts`         | Regenerates `web/src/generated/contracts.ts` from the Rust contract types. A stale file fails `test` and `check`.                                                                                                                                                     |
| `bun run audit`             | `cargo deny check` (advisories, licenses, bans, sources) and `zizmor` over the workflows. Needs network for the advisory database; runs as a separate CI job.                                                                                                         |

The pre-commit hook ([`lefthook.yml`](lefthook.yml)) runs a fast read-only subset and never rewrites or stages files. Fix findings with `bun run fmt` or `bun run contracts`.

## Changes

- Keep a change to one concern; each commit should pass `bun run check`.
- Commit subjects follow lightweight [Conventional Commits](https://www.conventionalcommits.org/) (`feat(core): …`, `fix(api): …`, `docs: …`).
- Behaviour changes need tests that exercise the real path: owned fixture processes, the real HTTP server, a real browser. Tests must never signal a process they did not start.
- Fixture processes come from `stackscan-fixtures` (`crates/fixtures`): `Fixture::spawn` launches the `stackscan-fixture` binary, waits for its ready line, records PID, start time, cwd and endpoints, and cleans up through its own child handle after an identity check. A fixture also exits on stdin EOF (its owner died) and after `--max-lifetime-secs`. `stackscan_fixtures::lsof` gives independent ground truth. Run fixture tests with `cargo nextest run --workspace <filter>`: with `-p` alone they fail on a clean target, or run a stale fixture binary on a warm one.
- If a change alters a contract in `blueprint/`, update the smallest relevant section in the same change and say why.
- Security-sensitive areas (HTTP guards, redaction, process signalling) get a test for the failure case, not only the happy path.

Security issues: see [SECURITY.md](SECURITY.md); please do not open public issues for them.

## Running the binary

`target/release/stackscan serve [--port <n>] [--no-open]` serves the embedded UI and API on `127.0.0.1` (default port 47831) until SIGINT/SIGTERM. An occupied port exits 1 with a clear message; usage errors exit 2.

`stackscan list` scans once and prints one row per listening port (PID, process, port, project and package, branch, bind addresses), with warnings on stderr. `stackscan list --json` prints the same scan as the `Snapshot` contract (`web/src/generated/contracts.ts`): revision 1, the observation (services, projects, warnings) and the scanner status. A one-shot scan sees each process once, so CPU reads `warmingUp`; `serve` samples it from the second scan on. Both exit 0 with partial data (warnings) and 1 if sockets cannot be enumerated at all. Until a resident instance exposes its snapshot (S04/S08), `list` always runs its own one-shot scan.

Project attribution runs `git` from the active developer directory (`xcode-select -p`); without Xcode or the Command Line Tools, services inside Git worktrees are reported with an `attributionUnavailable` warning instead of a project.

`serve` runs the instance's single scanner loop (3 s cadence) on its own thread and logs scan failures, recoveries and overruns to stderr; its snapshots are not served over HTTP until S04.

The rest of the [command contract](blueprint/architecture.md#configuration-and-packaging) arrives with later slices: tray mode with S08; `doctor`, `startup` and the release/install commands with S09.
