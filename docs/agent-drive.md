# Driving the real app from an agent

`bun run app` starts the real menu-bar app (tray + dashboard window, the shipped `kosse window` WKWebView) on a fictional fixture world and gives an agent one command each to read and drive the window. It exists because Playwright drives its own WebKit build at its own sizes, never Kossé's window: in S11 the window drew every modal as a 2px sliver behind a visible backdrop while Playwright saw them fine ([S11](evidence/S11.md)). It is for seeing a change through the actual entry point; `bun run check` stays the gate and `bun run test:window` is the automated suite on the same channel.

```sh
bun run app -- start                 # build what is stale, start app + window, print status JSON
bun run app -- status                # session, window URL, title, size, visibility, focus, app name
bun run app -- eval "document.title" # code in the real WKWebView; await works; prints JSON
bun run app -- shot                  # PNG of the window's page (var/app/shot.png)
bun run app -- resize 560x420        # content size in points (minimum 560x420, default 1180x780)
bun run app -- click 'li[data-row] button' Stop   # a real AppKit click (selector [+ exact text], or x y)
bun run app -- key Escape            # a real key press: j k / Enter Escape Tab Backspace arrows, a char
bun run app -- theme light           # window appearance (the page's prefers-color-scheme): light|dark|system
bun run app -- show stop:<pid>.<start>  # POST /api/window, the tray's Inspect/Stop… path; <start> = startedAtSecs (status lists it)
bun run app -- logs [--follow]       # the app's and window's output
bun run app -- opened                # URLs the app handed to "the browser" (captured)
bun run app -- stop                  # quit the app, release the fixture world
```

## What runs, and what it isolates

- **A detached host** (`scripts/app.ts host`, recorded in `var/app/session.json`) owns everything below and releases it on `stop`. `stop` signals only that host, after checking its command line carries this checkout's marker; the host stops its own children through their handles. If the host died uncleanly, its fixtures exit with their stdin and `stop` releases the app only when its PID, start time and executable still match the session.
- **A fixture world** (`e2e/support/world.ts`, the README screenshots' showcase variant): temporary Git projects under `/tmp/kosse-e2e-*` (`storefront`, a linked `checkout-redesign` worktree, `billing-api`, and `marketing-site` with a central profile) with owned `kosse-fixture` listeners named `vite`, `node`, `astro`, `next-server` (busy, ignores SIGTERM), `queue-worker`, `postgres`, `caddy`, and in `marketing-site` a second `vite` (its site plus an `Inspector` details endpoint) and `workerd` (two ports answering 500, all details through `otherEndpoints`). They exit with the host, or after 6 h.
- **The debug `kosse` app** on this checkout's own port (48100–48899, derived from the checkout path; never 47831, the installed default, or 47832/47833, `bun run dev`), with `--probe-only` and `--act-only` naming only the fixtures. It shows every listener on the machine, the owner's real services included, but it can neither probe nor stop them (`outsideTestScope`). An instance the owner runs on another port is never touched; one on the same port is refused by the usual instance check.
- **Its own configuration** (`KOSSE_CONFIG_DIR` inside the world, removed with it) and a capturing opener (`KOSSE_TEST_OPENER`, appending to the file `KOSSE_APP_OPENED` names: `var/app/opened.log`), so Open and service links never start a browser.
- **The control socket**: the window process listens only in a debug build with `KOSSE_WINDOW_CONTROL=1` and `KOSSE_WINDOW_CONTROL_SOCKET=<path>` (`crates/app/src/window_control.rs`). Release builds contain none of it (checked with `strings`), and an installed app never listens. The socket is `/tmp/kosse-app-<uid>/<checkout hash>.sock` (macOS limits a socket path to 104 bytes), owner-only (0600, directory 0700). One JSON request per connection (at most 1 MiB, 5 s to read and write; the socket is removed when the window closes): `status`, `eval`, `snapshot` (`WKWebView.takeSnapshot`, no screen-recording permission; written only as a `.png` under `KOSSE_WINDOW_CONTROL_DIR`, `var/app` here, or the temporary directory, never through a symlink), `resize`, `click`, `key`, `theme`, `quit` (closes the window; `show` opens a new one).

`start --hidden` runs the window invisible (`KOSSE_TEST_WINDOW=headless`): DOM and layout reads work, but WebKit throttles timers and rendering (shots are stale) and AppKit ignores clicks there. `bun run test:window` uses `KOSSE_TEST_WINDOW=transparent` instead: a shown window at 1% opacity, above other windows and click-through, which renders and takes the socket's clicks while the person sees and hits nothing. (A window placed off every display does not stay there: AppKit pulls a titled window back onto a screen when it is shown or resized.) Both commands refuse to start while the screen is locked: macOS then occludes every window and WebKit treats the page as hidden.

## The preview identity

Every debug build (`bun run app`, `bun run dev`, all tests) is a preview, so it is never mistaken for the installed app: the window is titled `Kossé · preview (<branch>)` (the checkout's branch, read from its `HEAD` file by `crates/app/build.rs` at build time), the tray and the window's Dock / ⌘-Tab icon use the split-stem K preview artwork (`brand/tray/tray-preview*.png`, `brand/Kosse-preview.iconset`), and the tray tooltip names it the same way. Release builds show `Kossé` and the standard icons. A bare executable still appears as `kosse` in the app menu, ⌘-Tab and Activity Monitor: those names come from a bundle's `Info.plist`, which Kossé does not have.

## Writing checks with it

- `eval` takes an expression or a function body with `return`; promises are awaited (the result is parked in a page global and read back). An element comes back as its HTML.
- The visible window renders normally, so `await new Promise(r => setTimeout(r, 300))` is fine there. In hidden or transparent use, poll from outside with separate `eval` calls, as `scripts/window-test.ts` does.
- WebKit queues key and mouse events separately: after typing, wait for the field's value before a click.
- Pick fixture rows by port (`li[data-row][aria-label^="postgres, port 5432"]`): another world or the machine's own services can show the same names.
- A dialog is visible when its rect is inside the viewport, larger than a sliver, fully opaque, and `elementFromPoint` at its centre lands inside it. `document.documentElement.scrollWidth <= innerWidth` is the no-horizontal-scroll check.

## Safety

The window opens on the owner's desktop: keep runs short and `stop` when done. Never confirm Stop or Force on anything but a fixture (the debug app refuses others anyway). Screenshots show the machine's real listeners: before committing one, filter the dashboard to the fixture projects (search `storefront`) and check it. Rollback: `bun run app -- stop`, then remove `var/app/` and `/tmp/kosse-app-<uid>/`.
