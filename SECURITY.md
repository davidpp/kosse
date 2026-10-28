# Security policy

## Reporting

Please report vulnerabilities privately through GitHub's [private vulnerability reporting](https://github.com/davidpp/stackscan/security/advisories/new) rather than a public issue. Include the Stackscan version (`stackscan --version`), macOS version, and reproduction steps. Expect an acknowledgement within a few days; fixes ship in the next release.

Only the latest release (or `main` before the first release) is supported.

## Threat model

These defenses are the v1 design contract; before the first release they land slice by slice, and each slice's evidence in [`docs/evidence/`](docs/evidence/) states what is implemented and tested.

Stackscan runs as the current user, binds only to loopback, and can send signals to that user's processes after explicit confirmation. Its defenses target:

- **Websites in the user's browser**: DNS rebinding and cross-site requests. Every route validates `Host`; unsafe methods require an exact same-origin `Origin`, a JSON content type, a bounded body and a per-instance action token fetched only through the same-origin API. There is no wildcard CORS.
- **Untrusted repositories**: optional repo-local metadata is size-limited data. It cannot run commands, change network or system configuration, fetch remote resources, or relax action eligibility.
- **Accidental harm**: stopping a process needs a fresh PID and start-time identity, an own-user target and confirmation; force kill needs a second confirmation. Process groups, parents and Stackscan itself are never signalled.
- **Data exposure**: command lines are redacted centrally before reaching the API, UI or logs; environment variables are never exposed; nothing is sent off the machine.

Out of scope: malicious local processes running as the same user (they already hold the user's privileges), and the remaining PID-reuse window between identity check and signal, which is documented in [`blueprint/architecture.md`](blueprint/architecture.md#actions).

The full contract is in [`blueprint/architecture.md`](blueprint/architecture.md) and [`blueprint/verification.md`](blueprint/verification.md).
