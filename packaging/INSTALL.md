# Installing Kossé {{version}} (macOS on Apple silicon)

This archive holds one self-contained executable, `kosse`, with the dashboard built in. It needs
no Bun, Node or other runtime. It is **not signed or notarized** (only the linker's ad-hoc
signature, which Apple silicon requires); see "Gatekeeper" below.

## Check the download

Before extracting, next to the downloaded archive:

```sh
shasum -a 256 -c kosse-{{version}}-aarch64-apple-darwin.tar.gz.sha256
```

`MANIFEST.json` (inside the archive) lists the SHA-256 of every file in it and the commit it was
built from.

## Install

Installed with Homebrew? Skip this.

```sh
tar -xzf kosse-{{version}}-aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
install -m 0755 kosse-{{version}}-aarch64-apple-darwin/kosse ~/.local/bin/
```

If `~/.local/bin` is not on your `PATH` (it is not on a stock Mac):

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile
```

then open a new terminal, and check:

```sh
kosse --version
kosse doctor
```

From a source checkout, `bun run install:local [--prefix <dir>]` does the same after verifying
the checksums, records the files it installed and can remove exactly those again
(`bun run uninstall:local`).

## Use

- `kosse` runs the menu-bar app and opens the dashboard window (`--no-open` to skip the window).
- `kosse open` shows the dashboard without leaving anything running.
- `kosse serve` serves the dashboard at `http://127.0.0.1:47831/` for any browser.
- `kosse list` prints the listening ports once; `kosse doctor` checks the setup.
- `kosse startup enable` starts the menu-bar app at each login (`disable` to stop, `status`).
  It runs the `kosse` you enable it from, so use the installed one.

## Gatekeeper

An archive downloaded with `curl` and extracted with `tar` (or installed by Homebrew) carries no
quarantine attribute, so macOS runs this unsigned executable without asking. A browser download
is quarantined: macOS then refuses to open `kosse` because it cannot check it for malicious
software. Only if you trust the archive's origin and its checksum matches, remove the attribute
from the executable:

```sh
xattr -d com.apple.quarantine kosse
```

This turns off Gatekeeper's check for that file only. Prefer downloading with `curl -LO`.

## Uninstall

With Homebrew: `kosse startup disable && brew uninstall kosse`.

Otherwise:

1. `kosse startup disable` (if you enabled it).
2. Note where your saved profiles are: `kosse config path`.
3. Delete the executable: `rm ~/.local/bin/kosse`.
4. Delete the file from step 2 to remove your profiles too.

Kossé is MIT-licensed (`LICENSE`); the licenses of the code it includes are in
`THIRD_PARTY_NOTICES.md`.
