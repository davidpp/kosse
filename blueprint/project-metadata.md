# Optional project metadata

## Three levels

1. **Observed:** no config; discover listeners/processes and infer repository/package.
2. **Annotated (v1):** “Customize this project” saves service names/matches, endpoint intent, probe paths and links centrally in Kossé's per-user application config directory. Zero repository footprint is the default, including work repositories. Optional repo-local `kosse.json` is a shareable alternative, never required or written automatically.
3. **Extended (later):** opt-in integrations for LocalCan, Portless, tunnels, proxies or launch-time port allocation. External providers own those capabilities; Kossé presents their state and explicit actions. No plugin runtime or managed integration is required for v1.

Do not change an existing process's port: avoiding conflicts requires participating in launch or receiving a supported runtime registration. Metadata is not a port reservation. An unavailable desired port is reported, not repaired by killing its occupant.

## Proposed v1 schema

Finalize and generate JSON Schema in the metadata issue; this example is the portable project profile payload, not currently supported software. Central storage associates it with a canonical absolute worktree root in a machine-local envelope; a repo-local file derives its root from its location. Relative match paths use that root. No personal absolute path needs to be committed:

```json
{
  "$schema": "./kosse.schema.json",
  "version": 1,
  "name": "Acme",
  "services": {
    "web": {
      "name": "Frontend",
      "match": { "cwd": "apps/web", "port": 5173 },
      "protocol": "http",
      "links": [
        { "label": "Frontend via Portless", "url": "https://acme.localhost", "kind": "local" }
      ],
      "probePath": "/"
    },
    "api": {
      "name": "API",
      "match": { "cwd": "apps/api" },
      "protocol": "http",
      "probePath": "/health",
      "links": [
        { "label": "API docs", "url": "http://localhost:3001/docs", "kind": "local" }
      ]
    }
  }
}
```

Ship schema with the application and `kosse config schema` to emit it; do not require network access for schema resolution. Users choose where to save the emitted file. A schema URL/path is editor metadata only, never fetched by the daemon.

## Matching and precedence

- Central profiles are keyed by canonical absolute project/worktree path, never repo basename. Different worktrees stay separate. A moved project needs explicit reassociation; do not silently attach by name.
- Read central profiles and optionally a detected project's root file, each size-limited (64 KiB), no recursive home scan, includes, executable interpolation, scripts or env expansion.
- UI customization writes only to central storage. Repo export is an explicit opt-in action, not a side effect; no `.gitignore` or tracked-file edits.
- Relative cwd matches exact normalized path or descendants at directory boundaries; resolve symlinks and reject paths escaping the project.
- Port is optional and only narrows candidates; it never assigns ownership without a matching project/cwd.
- Service IDs are unique within project. Multiple matching entries produce a visible ambiguity, not arbitrary first-wins behavior. A cwd-only entry matching multiple processes groups/labels them but must not merge their destructive identities.
- Preserve detected repo/worktree/package evidence. Precedence: machine-local profile overrides repo-local profile, which overrides inference. Merge service entries by explicit service ID and scalar fields; replace arrays rather than silently concatenating. Support an explicit disabled flag for suppressing an inherited service entry. Show effective values and provenance. No profile can override PID/owner/bind observations.
- A declared service not observed is “configured, not observed,” never “running” or definitely “stopped.” Do not invent an action target.
- Reload metadata on bounded periodic mtime checks; invalid updates retain last-good config with clear invalid/stale warning. Test delete/recreate and schema-version mismatch.
- S06 outcome: the schema is generated from the parser's types (`kosse config schema`, committed as `docs/kosse.schema.json`); the validator in `crates/core/src/profiles/validate.rs` is the authority. `match` needs `cwd`, `port` or both and refuses unknown criteria (ignoring one would widen the match); `cwd` may not contain `..`. `match` merges as one value (mixing a central port with a repository directory would describe neither entry). Unknown fields elsewhere are kept on rewrite. Link `kind` defaults to `local` for loopback/`.localhost` hosts and `external` otherwise; every link carries its host and a `reach` of `loopback` or `declared`.
- v1 does not recursively discover configs in inactive repos. Saved central profiles can remain in a separate configured-projects view without a running listener; label them “not observed.” Repo-only absent-service rows exist while the project is known from active/briefly retained observations.

## Security

A cloned repository is untrusted input. Reading metadata cannot launch commands, install trust roots, enable tunnels, mutate system DNS, signal processes, or register startup services.
Only http/https links; render URL hostname visibly. Automatically verified local links are limited to loopback or .localhost. Custom domains (including LocalCan .local) may be saved explicitly as declared links, never auto-probed or assumed loopback based on their suffix. External/tunnel links are clearly labeled, never auto-probed and opened only on user action. No javascript/file/custom schemes or credentials in URLs.
Probe paths must be origin-relative paths without authority/scheme, bounded and validated; only apply to an already verified owned loopback endpoint. Declared protocol is a hint, not proof of HTTP health.
Destructive actions still require observed process identity and confirmation. Metadata cannot relax API security or eligibility.

## Existing tools and future adapter boundary

[Portless](https://github.com/vercel-labs/portless) documents named .localhost URLs, port allocation at launch, optional portless.json and HTTPS. Current default HTTPS setup can install a CA and elevate for privileged ports: never trigger automatically during discovery or tests. [LocalCan](https://www.localcan.com/docs/configuration/projects) describes declarative service targets/local domains, but its documented files are central under ~/.localcan/projects, not repo-local by default.

v1 can display an explicit friendly URL regardless of who manages it. Do not guess Portless's private registry format or shell out to activate it. A future read-only adapter must verify a supported API/CLI, retain provenance, degrade when absent, reconcile stale routes, and test dynamic target-port changes. Kossé must remain fully functional without it.

## Extension direction (not a v1 framework)

Keep observational core independent of providers. Future extensions may contribute endpoints, provenance, status and explicit actions without changing process identity or overriding safety policy. Start with one concrete integration before choosing an ABI, subprocess protocol, WASM, dynamic loading or an SDK; no empty abstractions now.

Tunnels/public exposure are opt-in per project/session. Show destination, exposure, provider, expiration when known, and a clear disconnect action. Credentials belong in OS secure storage/provider tooling, never repo metadata or snapshot JSON. CA trust, privileged bind, command launch and public exposure require explicit capability/consent boundaries. Provider failure must not break discovery. Distinguish imported/external resources from Kossé-created resources; disconnect/cleanup must never tear down unrelated provider sessions.

No custom proxy, CA manager, allocator, launch wrapper or generic plugin framework in v1. Scope the first managed integration in a separate epic after the observational product works.
