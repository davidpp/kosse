# Kosse — Le raccord

## Idea

Kosse asks what is running on your computer and connects local endpoints to their owning process. The name comes from Québécois “kossé ?”. Write **Kosse** in prose and **kosse** in packages and the wordmark.

« Kosse qui roule sur mon ordi ? »

## Mark

One stem is the process; its two arms are the endpoints it owns. The short upper arm and longer, planted lower arm make an asymmetric K. Flat caps, open wedges, no nodes. This is ownership, not a Git history.

All geometry comes from [tools/mark.mjs](tools/mark.mjs), on a 16-unit grid: stem x=1–5, y=1–15; junction (4,7); arm endpoints (10.75,2.5) and (14,13.5); arm width 3. Small exports use 3.6. Never substitute a font K. Keep one stem-width of clear space in layouts; the asset canvas itself is tightly fitted. Minimum mark size: 16 px.

## Wordmark

Lowercase **kosse**, Bricolage Grotesque Bold 700, outlined and single-colour. Manual pair adjustments: ko −0.025 em, os −0.018 em, ss −0.012 em, se −0.018 em. The supplied static font determines its optical design; no variable optical-size claim. Use the assets rather than retyping. The lockup combines the mark with the unchanged wordmark.

## Colour

**Fleuve** is the St. Lawrence’s deep green-teal: the Québec link lives in colour, without symbols. Light Fleuve is #006B68; dark Fleuve is #71D6CD. Neutrals carry a slight green tint. Canvas: #F4F6F3 / #121A19; surface: #FFFFFF / #1C2725. Full OKLCH values and sRGB hex are in [tokens.json](tokens.json).

Contrast below is the lower WCAG ratio against canvas or surface, calculated from sRGB. Focus aliases Fleuve visually; unknown uses the muted-text values. Colour always accompanies state shapes and labels.

| Role · name          | Light / dark hex  | Light contrast | Dark contrast |
| -------------------- | ----------------- | -------------: | ------------: |
| ink · Encre          | #202A2A / #E8EDEB |          13.55 |         12.99 |
| ink-muted · Cendre   | #566460 / #A5B3AE |           5.70 |          7.07 |
| line · Joint         | #858F89 / #64756F |           3.07 |          3.16 |
| line-strong · Arête  | #62736C / #899D94 |           4.61 |          5.35 |
| accent · Fleuve      | #006B68 / #71D6CD |           5.85 |          8.93 |
| listening · Graphite | #384E48 / #B7C9C2 |           8.22 |          8.89 |
| reachable · Bleu     | #235DB0 / #8DBBFF |           5.92 |          7.82 |
| stale · Ocre         | #865900 / #E5BB65 |           5.61 |          8.51 |
| failed · Brique      | #AD353D / #FF9BA2 |           5.76 |          7.66 |

Listening is an observation, not success. Reachable means an HTTP response, including errors, not health. Unknown stays readable. **Arrêt**, the danger token, aliases failed but names an action. Use red text in rows and a solid red button only for the final Stop confirmation.

Solid buttons use surface text in light mode and canvas text in dark mode. Accent: 6.36:1 / 10.28:1; danger: 6.26:1 / 8.81:1. Focus rings are 2 px with a 2 px background-coloured gap. Keep meaningful borders fully opaque.

## Type

Bricolage Grotesque is used only as outlined brand artwork. The dashboard uses system-ui for text and ui-monospace for ports, PIDs, paths and commands, with tabular figures for metrics. No bundled or requested web fonts.

## Tray

Only **default** and **attention**. The latter cuts a 2 × 2 pt notch from the lower stem at y=11, retaining 2 pt of stem. It means discovery failed or the snapshot is stale; an individual failed HTTP probe does not change the tray. The menu supplies the reason. Confirmation stays in the dialog.

The 18 pt template has black RGB and transparent alpha, a 14 pt tall glyph in a 16 pt inner grid, and whole-pixel stem and notch edges at 1×. macOS supplies the tint. No animation. The preview simulates tint on menu-bar greys; native template rendering remains to be checked in the app.

## Voice

Plain, specific and unhurried. Never infer “forgotten”, “safe” or “healthy” from observations alone. One short line of French is enough.

- Empty after a successful scan: “No listeners found. Start a local service and it will appear here.”
- Stop confirmation, illustrative values: “Stop node (PID 4821) in storefront? This will request a graceful stop and affect ports :5173 and :5174.” Buttons: **Cancel** / **Stop process**.
- Unknown cwd: “Project unknown. The working directory isn’t available.”
- About: “Kosse qui roule sur mon ordi ?”

## Assets

[README.md](README.md) lists the generated assets and regeneration command. The app icon is a white mark on a flat Fleuve superellipse, inset 100 px on its 1024 px canvas. The 16 and 32 pt iconset entries use heavier arms and a larger glyph. The favicon fills its canvas to preserve legibility. [preview.png](preview.png) shows the family at review sizes.
