# Kossé — Relay

Kossé makes local development services recognizable: which process owns each port, and which project owns that process. Its name comes from Québécois “kossé ?” (“what's that?”). Write **Kossé** in the interface and prose; **kosse** remains the executable and package name.

## Mark

Relay is a separated K: one rounded vertical stem beside two joined diagonal arms. The open joint is intentional; the symbol is independently drawn, never a font K. The large master is on a 100-unit grid: stem x=15–29, y=14–86 with 7-unit corner radius; arms join at (46,50), ending at (77,20) and (77,80), with 14-unit round strokes. Geometry lives in [tools/mark.mjs](tools/mark.mjs).

Use the large master for the app icon and lockup. Keep at least one stem-width of clear space around the visible mark. Small template icons use dedicated geometry below.

## Wordmark

**Kossé**, capital K and acute accent, in **Kibernetyk Mono SemiBold**. Retain the original monospace advances; no proportional kerning or letter-by-letter stretching. The outlined SVGs come from the [font](fonts/README.md) committed here, v1.0.1; interfaces draw those outlines and never load the font. The mono letterforms supply the terminal character without a prompt symbol, cursor, scanline or retro screen effect.

The font is SIL OFL 1.1; its copyright and full license ship in [fonts/OFL.txt](fonts/OFL.txt). The mark and original artwork remain MIT. The font is for the wordmark; body and interface text use the system faces.

## Colour

**Québec-inspired blue #003DA5** is the app tile and light-surface accent. **#91B3FF** is its accessible dark-surface companion. These are product colours; the hex is not claimed as an official digital flag specification. The government's visual guide specifies Pantone 293 for the fleurdelisé rectangles ([source](https://cdn-contenu.quebec.ca/cdn-contenu/gouvernement/travailler/programme-identification-visuelle/guides/section1_general.pdf)). No fleur-de-lis or governmental signature is used.

Surfaces are neutral: canvas #F7F7F5 / #18181A, surface #FFFFFF / #232326, ink #232326 / #EDEDEB. Blue carries selection, focus and primary actions. Red names destructive actions. Observation colours are distinct semantic roles and do not mean service health. An HTTP 404 still counts as reachable HTTP.

All roles live in [tokens.json](tokens.json), generated from [tools/palette.mjs](tools/palette.mjs). Text roles meet 4.5:1 against both canvas and surface; meaningful boundaries/focus meet 3:1. The validation command checks both appearances.

## Tray

The **Optical 18** master is drawn directly in an 18-point canvas. Its stem has whole-pixel edges x=2–5 and y=2–16; the 2.5-point diagonal arms meet at (7.25,9) and end at (14,3)/(14,15). The minimum joint gap is 1 point. This tightens the joint relative to the large mark so it reads as one K in the menu bar.

A separate 16-point master uses x=2–5 and y=2–14 for the stem, 2.5-point arms through (7,8), ending at (12.5,3)/(12.5,13). Do not create it by resizing Optical 18. The app uses the 18-point `@2x` PNGs; macOS supplies the template tint. RGB is black with transparent alpha, in both 1× and 2× exports.

**Attention** cuts a small notch into the inner lower stem. It means discovery failed or the shared snapshot became stale, not that one service returned an HTTP error. The menu supplies the reason; the icon does not animate.

## Preview identity

Debug builds use a **split stem**, visibly divided at the centre. The app icon also changes from a white mark on blue to a blue split-stem mark on a pale tile with a blue rim. It remains a K, not a mirrored release icon. The tray has separate preview and preview-attention templates, with the split and attention notch independently visible. Existing asset names preserve the compile-time debug/release boundary.

The window title also retains “preview” and the branch when available. Never use the release mark for a development process.

## Native app icon

The 1024 px master has a 100 px transparent outer margin and an 824 px flat superellipse tile. Small iconset entries use the optical mark rather than reducing the large glyph. The favicon fills its canvas for legibility. No gradients, glass, remote fonts or image dependencies are needed at runtime.

## Voice and evidence

Plain, specific, unhurried. Do not infer “forgotten”, “safe” or “healthy” from an observation. Stop confirms the exact process and all ports before an action. Unknown remains unknown.

[preview.png](preview.png) shows the generated family. [tools/validate.mjs](tools/validate.mjs) checks file dimensions, transparent template pixels, distinguishable states, the real accent glyph, font advances and palette contrast. Native status-item tint, Dock appearance and activation still require evidence from the actual app; the contact sheet does not establish those checks.
