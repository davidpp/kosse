# S12 — Native Rune dashboard

Verified locally on macOS arm64, 2026-10-29. Rune and rune-kit are pinned to upstream revision `b63efb3db3a2fa59bf4a11a947944de2a8b69303`.

## Delivered

The dashboard is a native Rune window compiled into the Kossé executable. The browser UI, WebKit dependency and frontend build are removed. The existing Rust owner remains the single scanner; the window consumes the same guarded API/SSE and action policy. The resident tray, foreground server and ephemeral `kosse open` reuse their owner's one window. Closing the ephemeral window closes its owner. HTTP `/` is a script-free information page.

Project/worktree grouping, live observations, search, inspector, Open, Try HTTP, Stop and explicit Force are native controls. Search includes process, redacted command, effective project name, branch, package and ports. Ended rows and disconnected states retain their identity rather than implying a new process is the old one.

Customization is a structured project/service editor, with separate scrolling details and a fixed save footer. Fields cover names, local matching rules, protocol, probe path, links and availability. Repository values remain inherited unless explicitly overridden; central saves do not rewrite repository files. Individual entry removal changes the draft; removing all customization requires separate confirmation. Existing service IDs are immutable.

Relay's separated K, optically drawn menu-bar templates, Québec-inspired blue and Kibernetyk Mono replace the previous identity. System interface fonts and mono process facts retain compact operational density. Both appearances and narrow layouts use the same native components.

## Verification

- `bun run check`: formatting, strict lint/type checks, **211 Rust tests**, doctests and a locked release build. Core identity, action, probe, profile, guard and lifecycle coverage remains in place.
- `bun run test:window`: the actual Rune/AppKit window, real HTTP owner and owned fixture processes. Native text entry survives live revisions; search finds command-only and customized project text; layouts at 1180×780, 800×600 and 640×440 work in both appearances. Inspect and all-port confirmation stay within the viewport. Service Open is captured through the supported debug opener hook.
- The window suite edits and saves a real central profile, types `apps/web` through native input, verifies repository inheritance without copying inherited fields into the override, and verifies the repository file is unchanged. It opens inherited fields at the narrow size. Stop and still-running → explicit Force operate only on fixtures started by the harness.
- `bun run test:package`: **20 packaged-artifact checks** through a temporary installation and sandbox, including installed CLI, guarded API, fixture discovery/probing/profile writes/Stop, forbidden probe refusal, resident tray lifecycle, upgrade/config preservation and uninstall. The release contains no window-driver hooks. This establishes packaged CLI/API and lifecycle behavior; the interactive window suite uses the debug build.
- `bun run audit`: Cargo advisory/source/license policy and offline workflow review passed. Rune's transitive licenses and bundled font licenses are included in the release notices.
- Brand generation/validation: all 52 generated assets reproduce byte-for-byte; template dimensions/transparency, distinct preview/attention states, font advances/accent glyph and declared palette contrast pass.

Tests use isolated configuration and owned fixture processes. They do not install login items, change the real user profile, or stop/probe unrelated services. Native window tests are transparent and click-through; `bun run app` was used separately for visible review and capture, then stopped.

## Visual evidence

The real native driver captured fixture-only views, with the filter set to `storefront`:

- [Dark dashboard](../screenshots/dashboard-dark.png) and [light dashboard](../screenshots/dashboard-light.png).
- [Service customization](../screenshots/customize-native.png) and [narrow inherited customization](../screenshots/customize-native-narrow.png).

A fresh independent Impeccable review prompted fixes to search parity, slash input, removal/inheritance semantics, viewport density, blue controls and design documentation. The follow-up review cleared those fixes with a ship disposition. The native system and surface brief now live in `DESIGN.md`, `PRODUCT.md` and `.impeccable/`.

## Limits of the evidence

OS accessibility-tree exposure and VoiceOver, physical pointer/hover feel, Dock/menu-bar optical appearance under real interaction, cooperative activation across applications, reduced-motion behavior and an actual login cycle remain unverified. Native input and rendered-element geometry checks do not establish these human/OS checks.

The release executable is approximately 21 MB. No controlled native idle CPU/private-memory benchmark has been recorded; the old WebKit measurements are historical. The unsigned archive and unpublished formula are local artifacts only: nothing was pushed, published, signed or installed into the real user prefix.
