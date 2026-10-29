---
name: drive-app
description: Start, drive and read the real Kossé menu-bar app and its Rune dashboard window from an agent, on a fictional fixture world. Use whenever a change touches the dashboard, the window, the tray routes or the app, to reproduce a UI bug in the real window, take a screenshot of it, resize it, click or type in it, or check what a stop, force or save did. Prefer it over hand-launching target/debug/kosse: the driver sends real native input to the real window.
---

# Drive the app

Full reference: `docs/agent-drive.md`. Everything runs on owned fixtures, an isolated config and this checkout's own port; the debug app cannot probe or stop anything else.

```sh
bun run app -- start        # builds what is stale; returns status JSON once the window answers
bun run app -- tree
bun run app -- click search
bun run app -- text storefront
bun run app -- click <element-key>
bun run app -- shot         # then Read var/app/shot.png
bun run app -- resize 640x440; bun run app -- theme dark; bun run app -- key Escape
bun run app -- stop         # always, when done
```

After changing Rust or bundled assets, `stop` then `start` (it rebuilds; the window does not hot-reload). `bun run test:window` runs the automated real-window suite (transparent click-through window, its own instance; needs an unlocked screen).

Rules:

- The window is on the owner's desktop: keep runs short, `stop` at the end.
- Confirm Stop or Force only on fixture rows (pick rows by port). The dashboard also lists the owner's real services; never act on them, and never commit a screenshot that shows them (search `storefront` first).
- Report what you saw through these commands as evidence; say what stayed unverified (Dock and menu-bar appearance, real pointer feel, a person's login session).
