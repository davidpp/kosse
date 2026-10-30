# Optional project metadata

## Three levels

1. **Observed:** no config; discover listeners/processes and infer repository/package.
2. **Annotated (v1):** “Customize this project” saves service names/matches, endpoint intent, probe paths and links centrally in Kossé's per-user application config directory. Zero repository footprint is the default, including work repositories. Optional repo-local `kosse.json` is a shareable alternative, never required or written automatically.
3. **Extended (later):** opt-in integrations for LocalCan, Portless, tunnels, proxies or launch-time port allocation. External providers own those capabilities; Kossé presents their state and explicit actions. No plugin runtime or managed integration is required for v1.

Do not change an existing process's port: avoiding conflicts requires participating in launch or receiving a supported runtime registration. Metadata is not a port reservation. An unavailable desired port is reported, not repaired by killing its occupant.

## v1 schema (example; the emitted schema and validate.rs are authoritative)

The JSON Schema is generated (`kosse config schema`, committed as `docs/kosse.schema.json`); this example is the portable project profile payload, not currently supported software. Central storage associates it with a canonical absolute worktree root in a machine-local envelope; a repo-local file derives its root from its location. Relative match paths use that root. No personal absolute path needs to be committed:

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

A Vite dev server with the Cloudflare plugin (S21): node listens on the site's port, its inspector and an internal port; its workerd child listens on ports that change on every start.

```json
{
  "version": 1,
  "services": {
    "web": {
      "name": "Storefront",
      "match": { "cwd": ".", "port": 3000 },
      "protocol": "http",
      "endpoints": {
        "3000": { "name": "Site", "probePath": "/" },
        "9229": { "name": "Inspector", "show": "details" }
      },
      "otherEndpoints": { "show": "details" }
    },
    "worker": {
      "name": "Worker runtime",
      "match": { "cwd": ".", "process": "workerd" },
      "otherEndpoints": { "show": "details" }
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
- S15 outcome: the coding agent in a repository authors the central profile (`kosse profile context|show|set|remove|check|prompt`, [agent surface](agent-surface.md)); Kossé validates, previews with the scanner's own matching and writes only through `CentralStore`. Each root's stored profile has a revision (a content hash; `none` without one). `kosse profile set|remove --expect <revision>` and `SaveProfileRequest.expectedRevision` are compared and swapped under the store's lock and refused with a typed conflict when another writer changed the profile; requests without one apply unconditionally, as before.
- S21 outcome (endpoints, issue #9): profiles still match whole processes; `endpoints` and `otherEndpoints` then describe each matched endpoint. See [Endpoints](#endpoints-names-and-display-tiers-s21).
- v1 does not recursively discover configs in inactive repos. Saved central profiles can remain in a separate configured-projects view without a running listener; label them “not observed.” Repo-only absent-service rows exist while the project is known from active/briefly retained observations.

## Endpoints: names and display tiers (S21)

A dev stack's process listens on more than what a person opens (an inspector, internal ports), and a child such as workerd listens on ports that change on every start. Every endpoint stays visible; a profile says which ones are the service's launch targets.

- `endpoints`: an object keyed by port (decimal `1`–`65535`, no sign or leading zero, at most 64), each entry with at least one of `name` (label rules), `show` (`summary` | `details`; a listed endpoint without it is `summary`) and `probePath` (the service's rules). It merges central over repository as one value, like `match`; unknown entry fields are kept on rewrite.
- `otherEndpoints`: `{ "show": "summary" | "details" }` for the matched processes' endpoints that `endpoints` does not list; one value, unknown fields kept. Nothing demotes an unlisted endpoint implicitly (as in Aspire and VS Code).
- `match.process`: the exact OS process name, as `ProcessRecord.name` and the PROCESS column show it (label rules, at most 64 characters; the kernel keeps 31 bytes). Never the command line, which is redacted and untrusted. `match` needs at least one of `cwd`, `port` and `process`, and every one given must hold; matching still never crosses projects. Among the entries matching one process, those naming its `process` take it from those that do not, so a directory-wide entry and a `workerd` entry do not make the child ambiguous; equally specific entries stay ambiguous, and `process` with `port` is no more specific than `process` alone. Specificity never overrides precedence: when a repository entry (its `match` from `kosse.json`) naming the process would take it from an entry whose `match` is central, the process is ambiguous, so a cloned repository cannot silently displace the user's own profile (its HTTP declaration, endpoint names, probe paths and Open). Builds before S21 refuse a profile with `process` (the safe direction).
- Display, per observed endpoint (`effective::display`): for a process matched to one entry, its `endpoints` entry (basis `endpoint`), else its `otherEndpoints` (basis `otherEndpoints`), each with that field's source; otherwise, and for unmatched or ambiguous processes, port 9229 is `Inspector` in details (basis `inspectorPort`, the default of Node `--inspect`, Wrangler `dev.inspector_port` and the Cloudflare Vite plugin's `inspectorPort`: the only inference); anything else is `summary` (basis `default`). The result is `Endpoint.display`. Display never changes identity, eligibility, binds or which endpoints are probed.
- Launch targets are a service's summary endpoints (`Service::launch_endpoints`, mirrored by `web/src/model/endpoints.ts`): tray Open, dashboard Open, `kosse url <service>` and `kosse wait <service> --http` choose only among them. An explicit port (`kosse url 9229`, `wait --port`) answers for any endpoint. A service whose endpoints are all details has no launch target: `kosse url <service>` exits 1 and names its ports.
- Health: an endpoint's own `probePath` applies to it on either tier; the service's `probePath` is the health path of its summary endpoints only (still narrowed by `match.port`). A details endpoint's HTTP errors never mark the service failed in the dashboard.
- Not adopted: command-line patterns, port ranges, protocol fingerprinting (more probes), a hidden or `ignore` tier (issue #9 requires these endpoints to stay visible), and the issue's proposed app/internal/debug roles.

## Security

A cloned repository is untrusted input. Reading metadata cannot launch commands, install trust roots, enable tunnels, mutate system DNS, signal processes, or register startup services.
Only http/https links; render URL hostname visibly. Automatically verified local links are limited to loopback or .localhost. Custom domains (including LocalCan .local) may be saved explicitly as declared links, never auto-probed or assumed loopback based on their suffix. External/tunnel links are clearly labeled, never auto-probed and opened only on user action. No javascript/file/custom schemes or credentials in URLs.
Probe paths must be origin-relative paths without authority/scheme, bounded and validated (RFC 3986 path and query characters only); they apply only to an owned loopback endpoint where a plain `GET /` already got an HTTP response, so an untrusted file cannot steer the first contact. Declared protocol is a hint, not proof of HTTP health: only the user's central profile can make an endpoint an HTTP probe candidate with it; a repository's `protocol` is shown, never acted on.
Destructive actions still require observed process identity and confirmation. Metadata cannot relax API security or eligibility.

## Existing tools and future adapter boundary

[Portless](https://github.com/vercel-labs/portless) documents named .localhost URLs, port allocation at launch, optional portless.json and HTTPS. Current default HTTPS setup can install a CA and elevate for privileged ports: never trigger automatically during discovery or tests. [LocalCan](https://www.localcan.com/docs/configuration/projects) describes declarative service targets/local domains, but its documented files are central under ~/.localcan/projects, not repo-local by default.

Endpoint labels and tiers (public documentation, compared 2026-10-30 for S21):

| Need | Tilt | Aspire | VS Code / Dev Containers | Compose / Kubernetes |
|---|---|---|---|---|
| What you open | `links`, `port_forward(name=)` | resource URL shown `SummaryAndDetails` | `portsAttributes.onAutoForward: openBrowser` | — |
| Visible but secondary | none (undeclared ports do not appear) | URL `DetailsOnly` | `silent` | — |
| Debug port | a link named "Debugger" | none | a `label` | a port `name` (`debug` by convention) |
| Health | `readiness_probe` on one port and path | `WithHttpHealthCheck(path, endpointName)` | — | a probe on a named port |
| Per-port label | `name` | endpoint `name` | `label` | `name` |
| Ports that change | Tilt allocates them | named endpoints, port allocated | command-line pattern, ranges, `otherPortsAttributes` | named ports |

No tool has a debug category: it is a name. Primary versus secondary is a display tier (Aspire's summary/details, VS Code's open/silent), which maps to Kossé's row versus Inspect. Health belongs to one endpoint, so `probePath` moved onto it. Kossé observes rather than allocates, which puts it where VS Code is: identify the process (by its OS name) and give unlisted endpoints one default (`otherEndpoints`, after `otherPortsAttributes`).

v1 can display an explicit friendly URL regardless of who manages it. Do not guess Portless's private registry format or shell out to activate it. A future read-only adapter must verify a supported API/CLI, retain provenance, degrade when absent, reconcile stale routes, and test dynamic target-port changes. Kossé must remain fully functional without it.

## Extension direction (not a v1 framework)

Keep observational core independent of providers. Future extensions may contribute endpoints, provenance, status and explicit actions without changing process identity or overriding safety policy. Start with one concrete integration before choosing an ABI, subprocess protocol, WASM, dynamic loading or an SDK; no empty abstractions now.

Tunnels/public exposure are opt-in per project/session. Show destination, exposure, provider, expiration when known, and a clear disconnect action. Credentials belong in OS secure storage/provider tooling, never repo metadata or snapshot JSON. CA trust, privileged bind, command launch and public exposure require explicit capability/consent boundaries. Provider failure must not break discovery. Distinguish imported/external resources from Kossé-created resources; disconnect/cleanup must never tear down unrelated provider sessions.

No custom proxy, CA manager, allocator, launch wrapper or generic plugin framework in v1. Scope the first managed integration in a separate epic after the observational product works.
