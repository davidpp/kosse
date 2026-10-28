# Evidence: persisted dashboard preferences

Recorded 2026-10-28 on macOS 26.6.2, Apple silicon (arm64), Rust 1.99.1, Bun 1.4.2, Playwright 1.64.1 (WebKit, Chromium). The tree sits on `a22dcc2` (S08); the commit that adds this file is its fingerprint. Every command below was run after the last source change. Closes the deferral recorded in S07 ("All listeners" was a per-page toggle).

## What was built

| Where                                     | What                                                                                                                                                                                  |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/core/src/contract.rs`             | `Preferences { allListeners }`, the wire type (`deny_unknown_fields`), in the generated contracts.                                                                                    |
| `crates/core/src/profiles/central.rs`     | `CentralConfig.preferences` (`StoredPreferences`: known fields plus the unknown ones kept on rewrite); `CentralStore::preferences` and `save_preferences` through the existing store. |
| `crates/app/src/preferences.rs`           | `GET /api/preferences` and `POST /api/preferences`; the store-error mapping is shared with `POST /api/profile` (`customize::store_failure`).                                          |
| `web/src/api/client.ts`, `preferences.ts` | `preferencesQuery` and `savePreferences` in the one typed client; `useAllListeners` drives the header toggle and both "Show all listeners" buttons.                                   |

## Decisions

- **Same file, same version.** Preferences live in `config.json` under `preferences`, part of config version 1: a file without them has the defaults, and S06–S08 builds already keep the key as an unknown field, so no version bump (which would make older builds refuse the file). Defaults are not written, so a profile save does not add the key.
- **Same store guarantees.** Saves take the store's advisory lock and atomic replace, keep every other part of the file (profiles, unknown top-level keys, unknown preferences) and refuse a corrupt, invalid or newer file, leaving it byte-for-byte (`409 configUnavailable`). A known preference of the wrong type makes the file invalid, as for any other field.
- **A read route, not the snapshot.** Preferences are not observations; `GET` reads the file (defaults when there is none, creating nothing). Other open pages do not follow a change until they reload.
- **The request is closed.** `POST` takes exactly `{allListeners}`; an extra field is `400 invalidPayload`, so the API cannot plant arbitrary keys in the config. It passes the common guard (Host, exact Origin, JSON, 16 KiB, token) and has its own one-per-second limit.
- **The toggle never waits.** The choice applies to the page at once. Saves are spaced a second apart, only the latest pending choice is sent, and a `429` (another page, or this one before a reload) is retried once after a second (the next scheduled save waits for that retry's slot).
- **Leaving the page with a choice unsent.** A choice is only pending when a save was accepted less than a second ago, so a `keepalive` flush on `pagehide` would always get `429`. Instead, `pagehide` (or unmount) hands the pending choice to `sessionStorage`, and the next load of that tab starts from it and saves it. Closing the tab within that second loses it; sessionStorage being unavailable (private window, blocked site data) loses it too.
- **Read failures.** It shows "Saved preferences could not be read: …" and starts from the default; it is retried when the event stream is live again or another instance answers, except `configUnavailable` (a file to fix). A failed save shows "“All listeners” applies to this page only; it was not saved: …" (dismissable).
- **"Show all listeners"** (empty search, hidden matches) sets the same preference, since it checks the same toggle.

## Commands and results

| Command                                                  | Result                                                                                                                          |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `bun run check`                                          | Pass: fmt, oxlint, clippy, tsc, Vitest 37/37, vite build, budget, nextest **204/204**, doctests, release build; tree unchanged. |
| `bun run budget`                                         | **49.26 kB of 50 kB** gzipped (was 48.56 kB; +0.70 kB). No trimming needed; Lucide icons stay as imports.                       |
| `cargo nextest run --workspace -E 'binary(preferences)'` | 2/2 (below).                                                                                                                    |
| `cargo nextest run --workspace -E 'test(central::)'`     | 8/8, including the new `preferences_default_persist_and_keep_what_this_build_does_not_know`.                                    |
| `bun run test:e2e`                                       | **28 passed**, 2 skipped (screenshots run on WebKit only), 1.4 min.                                                             |

Real binary (`kosse serve --port 0`, temporary `KOSSE_CONFIG_DIR` under a sandbox, never `~/Library/Application Support/Kosse`):

- **Persist and reload:** `GET` gives `{allListeners: false}` and creates nothing. Missing token, a cross-site Origin and `text/plain` are refused (403/403/415); `"yes"` and an extra `theme` field are `400`; nothing is written. Against a hand-written file with an unknown top-level key, an unknown preference and a profile, `POST {allListeners: true}` → 200, a second save within the second → `429` + `Retry-After: 1`; the file is exactly the original plus `allListeners: true`, mode `0600`. After a restart on the same directory, `GET` gives `true`; saving `false` returns it.
- **Unusable file:** truncated JSON, `allListeners: "yes"` and `version: 9` each give `409 configUnavailable` for `GET` and `POST` (the message names `config.json`), and the file is byte-for-byte unchanged.

Browser (both engines; the world's own `KOSSE_CONFIG_DIR`):

- The search test's "Show all listeners" now persists, so it unchecks the toggle and waits for the save; later tests start from the default.
- **"All listeners is saved centrally and kept on reload, repository untouched":** check → `POST` 200 → reload → still checked and Kossé's own listener visible; `config.json` has `"allListeners": true`; the storefront repository's file fingerprints and `git status --porcelain` are unchanged. Uncheck → reload → unchecked, listener hidden. Check (saved), uncheck and reload within the second → the new load shows unchecked and saves `false`; a further reload stays unchecked. Then `config.json` is replaced by a `version: 9` file: reload shows "Saved preferences could not be read", checking the toggle still reveals the listener and shows "applies to this page only; it was not saved", and the file is unchanged (restored afterwards).

## Unverified or deferred

- **Kossé's own window** (WKWebView) was not exercised separately; it loads the same page and API.
- **Read retry on reconnect** is covered by code review only; no browser case drops `GET /api/preferences` alone.
- **Cross-page sync:** a second open page keeps its value until reload (no preference events).
- **Binary size** change not measured (one small handler and serde type).
