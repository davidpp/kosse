# Kossé brand assets

Relay is Kossé's original separated K, paired with Kibernetyk Mono SemiBold and Québec-inspired blue. [BRAND.md](BRAND.md) is the design specification; [preview.png](preview.png) shows the generated family.

## Assets and licenses

| Files                                                                   | Source and license                                                                               |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `kosse-mark*.svg`                                                       | Original Relay geometry; MIT                                                                     |
| `kosse-wordmark*.svg`, `kosse-lockup*.svg`                              | Original lockup and outlines from Kibernetyk Mono SemiBold; source font is OFL 1.1               |
| `app-icon*.svg`, `app-icon*-1024.png`, `Kosse*.icns`, `Kosse*.iconset/` | Relay on the generated blue tile; preview uses split stem/pale tile; MIT                         |
| `tray/tray-{default,attention,preview,preview-attention}*`              | Dedicated Optical 18 template geometry, 1×/2× PNG and SVG; MIT                                   |
| `tray/tray-16*`                                                         | Dedicated 16-point template geometry; MIT                                                        |
| `favicon.svg`, `favicon-32.png`                                         | Optical Relay in a full-canvas tile; MIT                                                         |
| `tokens.json`                                                           | Neutral/blue semantic colour roles; MIT                                                          |
| `fonts/KibernetykMono-SemiBold.ttf`                                     | Unmodified Kibernetyk Mono v1.0.1 by Pavlo Kilko / Project Authors; [SIL OFL 1.1](fonts/OFL.txt) |
| `tools/`                                                                | Generator, pinned dependency manifest/lock and validator; original code MIT                      |

Font provenance and exact SHA-256 are recorded in [fonts/README.md](fonts/README.md). Generated SVG metadata and PNG EXIF descriptions name the source generator and license. No font is installed globally by these commands.

## Regenerate and validate

Requires the repository's Bun toolchain and macOS `iconutil`. Build dependencies are pinned in `tools/package.json` and `tools/bun.lock`; no external `BRAND_TOOLS` directory is needed.

```sh
bun install --cwd brand/tools --frozen-lockfile --ignore-scripts
bun brand/tools/generate.mjs
bun brand/tools/validate.mjs
```

The generator writes only the assets in `brand/` and `tokens.json`. It reads the committed font directly and uses `harfbuzzjs` 1.6.4 for OpenType shaping, `opentype.js` 1.3.4 for outlines and `sharp` 0.34.5 for rasterization. The shaping step preserves the font's composed accented letter. `iconutil` builds both ICNS files from the retained iconsets. A conversion failure fails the command. The app itself does not need these tools.

Edit `tools/mark.mjs` for geometry, `tools/palette.mjs` for colour, and `tools/generate.mjs` for composition. Keep this manifest and `BRAND.md` current when those choices change. `preview.png` uses system Helvetica for specimen labels, so those labels can vary slightly with the host OS; the wordmark is always outlined from the pinned font.

The Rust embed paths (`crates/app/src/tray.rs`, `activation.rs`) are unchanged: release and debug compile different assets, and neither chooses the other identity at runtime. The app uses the Optical 18 `@2x` tray templates; `tray-16*` is a sized master for other 16-point uses, and the dashboard header draws its geometry inline. The dashboard shows the wordmark as an inline SVG of the outlines in `kosse-wordmark.svg`; the font file itself is a generator input only and is never served, bundled or embedded in the binary, so release notices do not carry it. Outlined artwork is not a Font Software redistribution under the OFL; the provenance stays in each SVG's metadata.
