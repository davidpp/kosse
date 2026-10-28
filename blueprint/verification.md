# Verification and release gates

## Fixture harness

Build controlled child processes with an ownership manifest containing PID/start identity, cwd, endpoints, launch time, and cleanup state. Temporary repositories/config/prefixes are disposable; real services are not. Use free ports, collision retries and bounded waits, not hard-coded sleeps as correctness gates. Cleanup checks identity before signaling and runs even on failure.

Scenarios:
- HTTP 200 with title, HTTP 404 API, 500, hanging response, huge response, redirect to external destination (must not follow).
- IPv4, IPv6-only where supported, dual stack, one process/multiple ports, non-HTTP listener.
- Repo with nested package, two same-name repos, worktree .git file, detached HEAD, path spaces/Unicode.
- Process exits during scan, PID/start mismatch (deterministic injected test), metadata permission unavailable.
- TERM exits; TERM ignored then explicitly forced; supervisor respawns a child. Only owned fixtures are signaled.
- Command strings with secrets/HTML; config corruption/newer version; SSE disconnect/reconnect; busy default port.
- Browser request attacks against Host/Origin/token and malformed actions.
- Scanner failure preserves last-good data with stale warning rather than false empty success.

## Evidence per boundary

| Boundary | Required proof |
|---|---|
| Discovery | Real fixture PIDs/ports match manifest and independent lsof output; no duplicates; metadata gaps explicit |
| Attribution | Deterministic fixture assertions plus real CLI JSON showing repo/worktree/package distinctions |
| Metrics | Samples from a controlled workload; warm-up unknown then meaningful CPU/memory; no fabricated zero |
| API/actions | HTTP integration tests of security and stale identity; owned process termination and respawn observation |
| Browser | Playwright against actual Rust server; open/inspect/filter/confirm-stop; reload, errors, reconnect; screenshots |
| Tray | Real packaged app starts a native menu; open/inspect/stop entrypoints verified via automation or human checklist |
| Packaging | Clean sandbox install→launch→discover→stop fixture→quit→uninstall; runtime needs no frontend toolchain |
| Startup | Generated launch configuration tested in sandbox; actual login cycle requires explicit owner approval/human evidence |

Unit/mocked tests are necessary but never sufficient for entry-point completion. A skipped IPv6 or GUI test must state platform reason and whether it blocks target acceptance.

## Security baseline

No destructive GET, no cross-origin mutation, no rebinding Host accepted, no raw secrets in JSON/logs, no shell interpolation, no arbitrary remote fetch, no signaling another user/self/stale identity. Render untrusted metadata as text. Offline runtime should not contact update/analytics/CDN endpoints. Audit dependency licenses/advisories at scaffold and release; blocked findings require recorded disposition.

## Evidence storage

Keep concise durable summaries in `docs/evidence/<issue>.md` when created; attach through `kabane log`. Include command, version/commit or working-tree fingerprint, environment, result, failures and unverified items. Generated screenshots/traces/artifacts remain ignored unless a small curated artifact is useful. Never copy private real process command lines into repository evidence.

## Completion audit

All required commands execute successfully from documented checkout instructions. CI configuration matches local checks (actual remote CI run remains unverified until a remote exists). All required children done with evidence. Installed entrypoint is tested, not only cargo run/Vite. No uncontrolled processes, live startup changes, or temporary fixture directories left behind.

Human-only tray/login verification cannot be silently marked passed by an unattended loop. Record waiting and provide exact steps. David may explicitly waive or defer a criterion; record that decision rather than changing acceptance unilaterally.
