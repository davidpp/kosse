# Drive the native app

`bun run app` opens the real Rune window and menu-bar app on owned fixture processes with isolated central configuration. It uses AppKit mouse, wheel and keyboard events, not state injection.

```sh
bun run app -- start
bun run app -- status
bun run app -- tree
bun run app -- click search
bun run app -- text storefront
bun run app -- resize 640x440
bun run app -- theme light
bun run app -- shot
bun run app -- key Escape
bun run app -- scroll 500 300 -200
bun run app -- show inspect:<pid>.<startedAtSecs>
bun run app -- stop
```

`tree` returns the actual rendered Rune elements: stable keys, roles, text, rectangles, opacity and focus. `click` accepts an exact key or `x y`; `text` types into the focused input. `scroll` takes x, y and wheel delta (negative scrolls down). `show` uses the same guarded route request as the tray. `opened` reads captured service URLs. `logs` reads the owner and window output.

A detached host owns the fixture world (`scripts/support/world.ts`), debug app and opener capture. The app uses a checkout-specific port, `--probe-only`/`--act-only` fixture guards and isolated `KOSSE_CONFIG_DIR`. `stop` verifies the recorded host identity and releases its children through their handles. It does not touch the installed app.

The debug-only control socket is owner-only (0600 in a 0700 directory). Requests are bounded and timed out. Screenshots use macOS window capture, so screen-recording access may be required; PNG output is restricted to the configured artifact directory or temporary directory and refuses symlink destinations. There is no JavaScript evaluation or web view.

`start --hidden` and `test:window` render a click-through window at 1% opacity. Native events still pass through the actual controls. Both require an unlocked screen. Release builds carry no driver or test hooks.

After a Rust/asset change, stop then start to rebuild. Before any public screenshot, search `storefront` and inspect the image: the app also observes real local listeners, which must never appear in committed evidence. Only act on fixture identities. Keep desktop runs short and stop when finished.

Debug icons use a split-stem Relay mark and a pale app tile; releases use the solid stem and blue tile. Human checks still cover menu-bar tint, Dock appearance, activation between applications, VoiceOver and physical pointer/trackpad feel.
