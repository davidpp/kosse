# Contributing

Kossé ships one Rust executable: a macOS menu-bar owner, a native Rune window and the CLI. Rune is pinned to a Git revision in Cargo.toml. There is no web build.

Install the tools pinned in `mise.toml` and `rust-toolchain.toml`, then run `bun install --frozen-lockfile`. Bun runs development and verification scripts; it is not needed by the installed app.

| Command                                     | Purpose                                                                                |
| ------------------------------------------- | -------------------------------------------------------------------------------------- |
| `bun run dev`                               | Build and run the native app on this Mac                                               |
| `bun run app -- start`                      | Open a preview on owned fixtures and isolated configuration                            |
| `bun run app -- stop`                       | Close that preview and release its fixtures                                            |
| `bun run check`                             | Non-mutating formatting, lint, types, Rust tests, contract freshness and release build |
| `bun run test:window`                       | Real Rune/AppKit interaction suite; unlocked screen required                           |
| `bun run contracts`                         | Regenerate `contracts/contracts.ts` and the profile JSON schema                        |
| `bun run fmt`                               | Apply Rust and repository formatting                                                   |
| `bun run audit`                             | Dependency/license/source and workflow audit                                           |
| `bun run release:local`                     | Build an archive, checksum, notices and unpublished formula                            |
| `bun run test:package`                      | Install, exercise, upgrade and uninstall the archive in an isolated sandbox            |
| `bun run install:local -- --prefix <dir>`   | Install only recorded archive files at an explicit prefix                              |
| `bun run uninstall:local -- --prefix <dir>` | Remove only the recorded installation                                                  |

`cargo nextest run --workspace <filter>` builds the fixture executable as well as the selected tests. Tests use real owned listeners and real HTTP; none may stop or probe a process it did not start. `KOSSE_CONFIG_DIR` isolates configuration. Keep window-producing tests under `KOSSE_TEST_WINDOW=headless` or `transparent`; `KOSSE_TEST_OPENER` captures service Open actions. Debug `--probe-only` and `--act-only` constrain the test owner to fixture identities. The release package test uses a seatbelt sandbox instead.

The native agent driver is documented in [docs/agent-drive.md](docs/agent-drive.md). The preview has a distinct split-stem icon and a `Kossé · preview (<branch>)` title. Release builds contain neither the preview identity nor the control socket. Never commit screenshots containing the machine's real services or private paths.

Startup tests use a temporary home and the debug-only `KOSSE_TEST_LAUNCHCTL`; they never alter a real launch agent. Packaging and installation tests preserve existing central configuration and never use the real installation prefix. Publishing, signing and login changes are separate actions.

Product, safety and architecture contracts remain in [blueprint](blueprint/README.md). Record user-entrypoint evidence and unverified checks under `docs/evidence/`.
