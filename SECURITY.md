# Security policy

## Reporting

Please report vulnerabilities privately through GitHub's [private vulnerability reporting](https://github.com/davidpp/kosse/security/advisories/new) rather than a public issue. Include the Kossé version (`kosse --version`), macOS version, and reproduction steps. Expect an acknowledgement within a few days; fixes ship in the next release.

## Supported versions

| Version        | Supported |
| -------------- | --------- |
| Latest release | Yes       |
| Older releases | No        |

Fixes land on `main` and ship in the next release; there are no backports.

## Verifying a release

Every [GitHub Release](https://github.com/davidpp/kosse/releases) carries the archive's SHA-256 file and a GitHub build-provenance attestation produced by the release workflow. Before installing:

```sh
shasum -a 256 -c kosse-<version>-aarch64-apple-darwin.tar.gz.sha256
gh attestation verify kosse-<version>-aarch64-apple-darwin.tar.gz -R davidpp/kosse
```

The checksum proves the download is intact; the attestation proves the archive was built by this repository's workflow from the tagged commit. The binary itself is not signed or notarized.

## Threat model

Kossé runs as the current user, binds only to `127.0.0.1`, and can send signals to that user's processes after explicit confirmation. Every defense below is implemented and tested; the evidence per slice is in [`docs/evidence/`](docs/evidence/README.md), the installed-binary audit in [`docs/evidence/S10.md`](docs/evidence/S10.md) and the native window in [`docs/evidence/S12.md`](docs/evidence/S12.md).

- **Websites in the user's browser** (DNS rebinding, cross-site requests). Every route, the script-free information page at `/` included, refuses a `Host` other than `127.0.0.1:<port>` or `localhost:<port>`. API requests a browser marks cross-origin (`Origin` or `Sec-Fetch-Site`) are refused, reads included. Every unsafe method (refresh, try HTTP, stop, force, window, profile, preferences) needs the exact same-origin `Origin`, `Content-Type: application/json`, a body of at most 16 KiB and the per-instance action token, which lives in memory only and is fetched through the same-origin API. There are no CORS headers. Every response carries a strict Content Security Policy (`default-src 'self'`, no inline or remote script, `frame-ancestors 'none'`), `nosniff` and `no-referrer`.
- **Accidental harm.** Stopping a process needs a fresh PID + start-time identity re-read just before the signal, an own-user target (real and effective UID), and confirmation naming every port; force kill is offered only after a graceful stop reported "still running", and needs a second confirmation. Process groups, parents, PID ≤ 1 and Kossé itself are never signalled, and nothing escalates automatically. The tray never signals: its Stop… opens the dashboard's confirmation.
- **Untrusted repositories.** A repository's `kosse.json` is passive data: read only at a detected project root, not through a symbolic link, at most 64 KiB, refused as a whole when any rule fails. It cannot run commands, include other files, fetch anything, declare a service HTTP, choose a probe path, or relax action eligibility. Links are validated as `http(s)`.
- **Configuration.** Only Kossé's own file (`~/Library/Application Support/Kosse/config.json`: central profiles and dashboard preferences) is ever written, under a lock, by temp file + `fsync` + rename, directory `0700` and file `0600`. A corrupt file or one from a newer version is never overwritten. It holds no token.
- **Other users on the same Mac.** Other users' processes are shown only as far as macOS reveals them, never probed and never signalled. A second launch reuses a running Kossé only after checking that the socket on the port belongs to the current user (before sending it anything), so no request or token goes to another user's instance; anything else on the port is refused with a message.
- **The dashboard window** is a native Rune child process (`kosse window`) with no web view and no script runtime. It talks only to its owner's guarded loopback API, after checking that the socket belongs to the current user, and sends the same `Host`, `Origin`, JSON and token as any other client. Process- and probe-derived strings are rendered as text. The only links it opens are validated `http(s)` URLs, in the default browser and only on a user action. `POST /api/window` is guarded like every unsafe route and only shows the one window at a dashboard route.
- **Data exposure.** Command lines are redacted centrally before reaching the API, the window or logs; environment variables are never read into the model; nothing is sent off the machine. Redaction is heuristic: it hides values of sensitive-looking keys, URL user information, `-u user:password` and well-known credential shapes, but a bare positional secret with none of these markers (for example `mysql -pSECRET`, or a random token as the only argument) is not recognised.
- **Probing local services.** Every 15 s Kossé sends one bounded `GET` (no credentials or cookies, redirects recorded and never followed, body capped at 64 KiB, connect and total timeouts) to each loopback endpoint of the current user's processes that run in a project, plus services the user's central profile declares HTTP. Other users' and system processes are never probed. HTTPS certificates are verified offline, in process, against the roots the user's keychains trust; verification never fetches anything and is never disabled.
- **Offline runtime.** Kossé makes no update, telemetry, CDN or name-resolution requests; it connects only to loopback (its probes and a second launch's instance check).

Out of scope: malicious local processes running as the same user (they already hold the user's privileges), and the remaining PID-reuse window between identity check and signal, documented in [`blueprint/architecture.md`](blueprint/architecture.md#actions). The binary is not signed or notarized; verify the archive's SHA-256 against a source you trust.

The full contract is in [`blueprint/architecture.md`](blueprint/architecture.md) and [`blueprint/verification.md`](blueprint/verification.md).
