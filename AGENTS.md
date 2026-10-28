# Stackscan — agent instructions

Stackscan observes local development services (listeners, processes, projects) and offers a browser dashboard and macOS tray menu over one Rust core. Contracts live in [`blueprint/`](blueprint/README.md); read the product, architecture and verification documents before changing behaviour, and the [implementation plan](blueprint/implementation-plan.md) slice you are working on. Commands and setup are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Commands

- `bun run check` — the full non-mutating gate (CI runs the same; the pre-commit hook runs a fast subset). Run it before claiming work is done.
- `bun run dev` — backend on `127.0.0.1:47832`, Vite on `127.0.0.1:47833` with a same-origin `/api` proxy.
- `cargo nextest run -p <crate> <filter>` runs one Rust test; `bun run --cwd web test <file-pattern>` runs matching web test files.
- `bun run contracts` regenerates `web/src/generated/contracts.ts` after a contract type changes. Never hand-edit generated files (`contracts.ts`, `routeTree.gen.ts`).
- `bun run fmt` and `bun run contracts` rewrite tracked files. Vite (`dev`, `build`, and so `check`) regenerates `web/src/routeTree.gen.ts` when routes change; commit it with the route. CI fails if `check` leaves the tree dirty.

## Invariants

- One scanner per running application. Views, request handlers and the tray read the shared snapshot; they never scan.
- All views use the same service model and action policy. No alternate destructive path.
- A destructive action needs a fresh PID + start-time identity and explicit confirmation. Never signal process groups, parents, other users' processes, or Stackscan itself.
- Bind loopback only. Unsafe HTTP methods require the Host/Origin/token checks in blueprint/architecture.md; never relax them for development beyond the documented dev authority.
- Every displayed or logged command line passes central redaction. Process-derived strings are untrusted text.
- `crates/core` has no UI, router or HTTP-server dependency.
- Rust: expected failures are typed `Result`s; no `unwrap`/`expect`/`panic` on OS data or process races. Use `#[expect(lint, reason = "…")]`, not `#[allow]`.
- TypeScript: strict, no `any`, `@ts-ignore` or unchecked casts (generated files excepted). TanStack Query owns server state; no fetching in components outside it.
- Small modules, no speculative traits or plugin layers. Add a dependency only when code uses it.

## Tests and evidence

- Test real behaviour through real entrypoints: owned fixture processes, the real HTTP server, a real browser. Mocks are only acceptable for pure domain logic or platform-provided test doubles.
- Tests never stop a process they did not start. Fixtures are recorded and cleaned up by identity, never by process name.
- A slice is done when its acceptance holds through the user-facing entrypoint. Record commands, results and anything unverified in `docs/evidence/<slice>.md`. Never mark a human-only check (tray on a real desktop, login startup) as passed.
- Do not weaken an assertion to get green; fix the cause or report the blocker.

## Boundaries

- Do not push, tag, publish, sign, install login items, or change global tool/agent configuration unless the maintainer asks.
- Keep public artifacts clean: no private absolute paths, personal tracker IDs, real process command lines, secrets, or conversation history in code, commits or docs. Plan slice IDs (`S04`) are fine.
- If implementation must change a blueprint contract, update the smallest relevant section and state why.
- Provenance: Stackscan is independently written from its blueprint and public library documentation. Do not consult or translate code from other port/process dashboards (e.g. PortDoc); see [blueprint/references.md](blueprint/references.md).
- UI work follows the design context in the web app and uses the Impeccable skill when available.
