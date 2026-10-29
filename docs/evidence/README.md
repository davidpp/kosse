# Slice evidence

One file per [implementation plan](../../blueprint/implementation-plan.md) slice: commands, environment, results and what stayed unverified. Slices marked _historical_ describe the browser dashboard and WKWebView window that S12 replaced; their non-UI results (discovery, API, actions, profiles, packaging) still hold.

| Slice                         | What it proved                                                                                        |
| ----------------------------- | ----------------------------------------------------------------------------------------------------- |
| [S01](S01.md)                 | Workspace scaffold and the one non-mutating gate shared by hook and CI (_historical_ web scaffold)    |
| [S01b](S01b.md)               | Slim web runtime within a 50 kB budget (_historical_)                                                 |
| [S02](S02.md)                 | macOS listener discovery matches `lsof`; the owned-process fixture harness                            |
| [S03](S03.md)                 | Project/worktree/package attribution, shared snapshots and CPU/memory sampling                        |
| [S04](S04.md)                 | Guarded loopback API, SSE and bounded HTTP probes                                                     |
| [S05](S05.md)                 | Identity-checked stop and explicit force on owned fixtures                                            |
| [S06](S06.md)                 | Central project profiles and read-only repository `kosse.json`                                        |
| [S07](S07.md)                 | Dashboard workflows against the real server (_historical_ web UI, Playwright)                         |
| [S08](S08.md)                 | Menu-bar app, `kosse open`, single instance; the human tray checklist (_historical_ WKWebView window) |
| [S09](S09.md)                 | Local archive, install/uninstall, opt-in login startup, `kosse doctor`                                |
| [S10](S10.md)                 | Installed-entrypoint acceptance audit of v0.1.0 (_historical_ for the dashboard and footprint)        |
| [S11](S11.md)                 | Driving the real window from an agent and the `test:window` suite (_historical_ WKWebView fixes)      |
| [S12](S12.md)                 | Native Rune window: real-window suite, packaged CLI/API, remaining human checks                       |
| [preferences](preferences.md) | The All listeners choice persisted in the central configuration (_historical_ web UI)                 |
| [rename](rename.md)           | Stackscan → Kossé rename                                                                              |
