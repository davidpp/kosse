<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/kosse-lockup-night.svg">
    <img src="brand/kosse-lockup.svg" alt="Kosse" height="56">
  </picture>
</h1>

A native macOS tray menu and a lightweight local dashboard for the development services you already run: **discover, identify, open, inspect, stop.**

You start dev servers from terminals, scripts and coding agents. A day later there are a dozen listeners on unremembered ports, some from worktrees you forgot. Kossé shows them grouped by repository, worktree and package, with honest observations (listening, reachable over HTTP, or neither) and current CPU and memory. It stops a forgotten process only after confirming its exact identity. It never asks you to register projects or change how you launch them.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/dashboard-dark.png">
  <img src="docs/screenshots/dashboard-light.png" alt="The Kossé dashboard: project cards for two checkouts of one repository and a package directory, each listing processes with their ports, what an HTTP probe saw (200, 404, an untrusted TLS certificate, a database that was not probed), uptime, CPU, memory and Inspect and Stop actions." width="900">
</picture>

<sub>Fixture processes from the browser suite (`bun run screenshots`).</sub>

> **Status: in development, not yet usable.** v1 targets macOS on Apple silicon. Progress is tracked slice by slice in the [implementation plan](blueprint/implementation-plan.md), with evidence for each slice in [`docs/evidence/`](docs/evidence/).

## Principles

- **Observe, don't orchestrate.** No launch wrappers, proxies, certificates or port allocation. Optional machine-local project profiles add names and links without touching your repositories; a repository may also ship an optional, read-only `kosse.json` ([schema](docs/kosse.schema.json), [example](blueprint/project-metadata.md#v1-schema-example-the-emitted-schema-and-validaters-are-authoritative)).
- **Observations, not health claims.** A `404` from `GET /` still means the server is reachable. A TCP database gets no fake "Open" link. Unknown stays unknown.
- **Safe by construction.** Stop targets a fresh PID + start-time identity after confirmation; force kill needs a second, explicit confirmation. No process groups, no parents, no other users.
- **Local only.** Loopback binding, Host/Origin/token guards against browser-based attacks, centralized secret redaction, no telemetry, no remote assets.

## Architecture

```text
crates/core   listener discovery · process identity · project attribution · snapshots · action policy · config
     │        (no UI, router or HTTP-server dependency)
crates/app    one scanner scheduler (+ bounded loopback HTTP probes) → Axum API + SSE (loopback) → embedded SPA · CLI · macOS tray
     │
web/          slim dashboard embedded into the binary, shown in a lightweight WKWebView window
              or any browser (TanStack Redact runtime, ≤ 50 kB gzipped JS + CSS)

crates/fixtures   test-only: owned listener processes with an identity manifest, disposable Git
                  repositories, lsof ground truth
```

Every view (dashboard, tray and CLI) reads the same snapshot and uses the same action policy. Rust contract types generate the TypeScript types the UI consumes, and CI fails on drift. Details: [architecture](blueprint/architecture.md), [product contract](blueprint/product.md), [verification gates](blueprint/verification.md).

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

Kossé is developed blueprint-first. [`blueprint/`](blueprint/README.md) holds the product contract, architecture, verification gates and a dependency-ordered slice plan. Each slice is implemented and verified through real entrypoints (owned fixture processes, the real HTTP server, a real browser), then reviewed independently for correctness and for documentation before it lands. Mocked tests alone never count as done.

## Brand

_Kossé ?_ is Québécois for "what's that?", the question you ask about an unfamiliar port. The mark, **Le raccord**, is a K whose one stem is the process and whose two arms are the endpoints it owns. Its colour is **Fleuve**, the St. Lawrence's green-teal. The identity, palette and tokens are in [`brand/BRAND.md`](brand/BRAND.md); the assets and how to regenerate them are in [`brand/`](brand/README.md).

## License

[MIT](LICENSE). Kossé is an independently written project; see [provenance notes](blueprint/references.md#inspiration-and-boundaries).
