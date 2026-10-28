# Security policy

## Reporting

Please report vulnerabilities privately through GitHub's [private vulnerability reporting](https://github.com/davidpp/stackscan/security/advisories/new) rather than a public issue. Include the Kossé version (`kosse --version`), macOS version, and reproduction steps. Expect an acknowledgement within a few days; fixes ship in the next release.

Only the latest release (or `main` before the first release) is supported.

## Threat model

These defenses are the v1 design contract; before the first release they land slice by slice, and each slice's evidence in [`docs/evidence/`](docs/evidence/) states what is implemented and tested.

Kossé runs as the current user, binds only to loopback, and can send signals to that user's processes after explicit confirmation. Its defenses target:

- **Websites in the user's browser**: DNS rebinding and cross-site requests. Every route validates `Host`; API requests that a browser marks cross-origin are refused, reads included; unsafe methods require an exact same-origin `Origin`, a JSON content type, a bounded body and a per-instance action token fetched only through the same-origin API. There are no CORS headers at all. Every response carries a strict Content Security Policy (`default-src 'self'`, no inline or remote script, `frame-ancestors 'none'`).
- **Untrusted repositories**: optional repo-local metadata is size-limited data. It cannot run commands, change network or system configuration, fetch remote resources, or relax action eligibility.
- **Accidental harm**: stopping a process needs a fresh PID and start-time identity, an own-user target and confirmation; force kill needs a second confirmation. Process groups, parents and Kossé itself are never signalled.
- **Data exposure**: command lines are redacted centrally before reaching the API, UI or logs; environment variables are never exposed; nothing is sent off the machine. Redaction is heuristic: it hides values of sensitive-looking keys, URL user information, `-u user:password` and well-known credential shapes, but a bare positional secret with none of these markers (for example `mysql -pSECRET`, or a random token as the only argument) is not recognised.
- **Probing local services**: `serve` sends one bounded `GET /` (no credentials or cookies, redirects recorded and never followed, body capped at 64 KiB) every 15 s to each loopback HTTP candidate owned by the current user and attributed to a project; other users' and system processes are never probed. HTTPS certificates are verified offline, in process, against the roots the user's keychains trust; verification never fetches anything and is never disabled.

Out of scope: malicious local processes running as the same user (they already hold the user's privileges), and the remaining PID-reuse window between identity check and signal, which is documented in [`blueprint/architecture.md`](blueprint/architecture.md#actions).

The full contract is in [`blueprint/architecture.md`](blueprint/architecture.md) and [`blueprint/verification.md`](blueprint/verification.md).
