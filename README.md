# Stackscan

A local homepage and native macOS tray menu for the development services you already run: **discover, identify, open, inspect, stop.**

You start dev servers from terminals, scripts and coding agents. A day later there are a dozen listeners on unremembered ports, some from worktrees you forgot. Stackscan shows them grouped by repository, worktree and package, with honest observations (listening, reachable over HTTP, or neither) and current CPU and memory. It stops a forgotten process only after confirming its exact identity. It never asks you to register projects or change how you launch them.

> **Status: in development, not yet usable.** v1 targets macOS on Apple silicon. Progress is tracked slice by slice in the [implementation plan](blueprint/implementation-plan.md), with evidence for each slice in [`docs/evidence/`](docs/evidence/).

## Principles

- **Observe, don't orchestrate.** No launch wrappers, proxies, certificates or port allocation. Optional machine-local project profiles add names and links without touching your repositories.
- **Observations, not health claims.** A `404` from `GET /` still means the server is reachable. A TCP database gets no fake "Open" link. Unknown stays unknown.
- **Safe by construction.** Stop targets a fresh PID + start-time identity after confirmation; force kill needs a second, explicit confirmation. No process groups, no parents, no other users.
- **Local only.** Loopback binding, Host/Origin/token guards against browser-based attacks, centralized secret redaction, no telemetry, no remote assets.

## Architecture

```text
crates/core   listener discovery · process identity · project attribution · snapshots · action policy · config
     │        (no UI, router or HTTP-server dependency)
crates/app    one scanner scheduler → Axum API + SSE (loopback) → embedded SPA · CLI · macOS tray
     │
web/          Vite + React + TanStack Router/Query + Tailwind v4 + shadcn/ui, embedded into the binary
```

Every view (browser, tray and CLI) reads the same snapshot and uses the same action policy. Rust contract types generate the TypeScript types the UI consumes, and CI fails on drift. Details: [architecture](blueprint/architecture.md), [product contract](blueprint/product.md), [verification gates](blueprint/verification.md).

## Development

Requires macOS on Apple silicon and [mise](https://mise.jdx.dev). All tool versions are pinned in the repository.

```sh
mise trust && mise install
bun install --frozen-lockfile
bun run check   # full non-mutating gate, same as CI
bun run dev     # backend + Vite on http://127.0.0.1:47833/
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for all commands and conventions, and [AGENTS.md](AGENTS.md) for the rules coding agents follow in this repository.

## How this project is built

Stackscan is developed blueprint-first. [`blueprint/`](blueprint/README.md) holds the product contract, architecture, verification gates and a dependency-ordered slice plan. Each slice is implemented and verified through real entrypoints (owned fixture processes, the real HTTP server, a real browser), then reviewed independently for correctness and for documentation before it lands. Mocked tests alone never count as done.

## License

[MIT](LICENSE). Stackscan is an independently written project; see [provenance notes](blueprint/references.md#inspiration-and-boundaries).
