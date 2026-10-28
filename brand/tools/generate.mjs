#!/usr/bin/env node
// All outputs stay in brand/. External tooling never becomes an app dependency.
import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { MARK, SMALL, markPath, attentionPath } from "./mark.mjs";
import { palette, colours } from "./palette.mjs";
const root = fileURLToPath(new URL("../", import.meta.url));
if (!process.env.BRAND_TOOLS)
  throw new Error("Set BRAND_TOOLS to an absolute directory containing the installed brand tools.");
const require = createRequire(process.env.BRAND_TOOLS.replace(/\/?$/, "/"));
const sharp = require("sharp");
const opentype = require("opentype.js");
const fontFile =
  require.resolve("@fontsource/bricolage-grotesque/files/bricolage-grotesque-latin-700-normal.woff");
const bytes = readFileSync(fontFile);
const face = opentype.parse(
  bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
);
const C = colours(),
  D = colours(true);
const write = (name, data) => writeFileSync(root + name, data);
const svg = (box, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${box}" role="img" aria-label="Kosse">${body}</svg>\n`;
const glyph = (colour, small = false, attention = false) =>
  `<path fill="${colour}" d="${attention ? attentionPath(small ? SMALL : {}) : markPath(small ? SMALL : {})}"/>`;
const mark = (colour, small = false) => svg("0 0 16 16", glyph(colour, small));
const raster = (source, size) => sharp(Buffer.from(source)).resize(size, size).png().toBuffer();
for (const [name, colour] of [
  ["", C.accent],
  ["-night", D.accent],
  ["-mono", "currentColor"],
])
  write(`kosse-mark${name}.svg`, mark(colour));
// Manual pair spacing, in em, in addition to the face's advances; no browser fonts.
const kern = [-0.025, -0.018, -0.012, -0.018];
let pen = 0;
const paths = [],
  bounds = [];
for (const [i, ch] of [..."kosse"].entries()) {
  const g = face.charToGlyph(ch),
    p = g.getPath(pen, 0, 200);
  paths.push(p.toPathData(3));
  bounds.push(p.getBoundingBox());
  pen += (g.advanceWidth * 200) / face.unitsPerEm + (kern[i] ?? 0) * 200;
}
const x = Math.min(...bounds.map((b) => b.x1)),
  y = Math.min(...bounds.map((b) => b.y1));
const w = Math.max(...bounds.map((b) => b.x2)) - x,
  h = Math.max(...bounds.map((b) => b.y2)) - y;
const word = (colour) => `<path fill="${colour}" d="${paths.join("")}"/>`;
const lockup = (accent, ink) =>
  svg(
    `0 0 ${w + h * 1.5} ${h * 1.2}`,
    `<g transform="scale(${(h * 1.2) / 16})">${glyph(accent)}</g><g transform="translate(${h * 1.5 - x} ${h * 0.1 - y})">${word(ink)}</g>`,
  );
for (const [suffix, col] of [
  ["", C],
  ["-night", D],
]) {
  write(`kosse-wordmark${suffix}.svg`, svg(`${x - 1} ${y - 1} ${w + 2} ${h + 2}`, word(col.ink)));
  write(`kosse-lockup${suffix}.svg`, lockup(col.accent, col.ink));
}
// Sampled superellipse, exponent 5, following icitte's parametric construction.
function squircle(inset, size, n = 5) {
  const a = size / 2,
    points = [];
  for (let i = 0; i < 360; i++) {
    const t = (i * Math.PI) / 180,
      c = Math.cos(t),
      s = Math.sin(t);
    points.push(
      [
        inset + a + a * Math.sign(c) * Math.abs(c) ** (2 / n),
        inset + a + a * Math.sign(s) * Math.abs(s) ** (2 / n),
      ]
        .map((v) => +v.toFixed(4))
        .join(" "),
    );
  }
  return `M${points.join("L")}Z`;
}
// macOS canvas: 100 px clear margin, 824 px tile. Glyph occupies 70% of the tile.
const icon = (small = false, favicon = false) => {
  const inset = favicon ? 0 : 100,
    tile = 1024 - inset * 2;
  const extent = tile * (favicon ? 0.84 : small ? 0.78 : 0.76);
  return svg(
    "0 0 1024 1024",
    `<path fill="${C.accent}" d="${squircle(inset, tile)}"/><g transform="translate(${(1024 - extent) / 2} ${(1024 - extent) / 2}) scale(${extent / 16})">${glyph("#FFFFFF", small)}</g>`,
  );
};
write("app-icon.svg", icon());
write("app-icon-1024.png", await raster(icon(), 1024));
write("favicon.svg", icon(true, true));
write("favicon-32.png", await raster(icon(true, true), 32));
const renderJobs = [];
mkdirSync(root + "tray", { recursive: true });
for (const state of ["default", "attention"]) {
  const tray = svg(
    "0 0 18 18",
    `<g transform="translate(1 1)">${glyph("#000000", true, state === "attention")}</g>`,
  );
  write(`tray/tray-${state}.svg`, tray);
  for (const scale of [1, 2])
    renderJobs.push(
      raster(tray, 18 * scale).then((data) =>
        write(`tray/tray-${state}${scale === 2 ? "@2x" : ""}.png`, data),
      ),
    );
}
// Retain the iconset as inspectable source for iconutil, with point-size-aware masters.
mkdirSync(root + "Kosse.iconset", { recursive: true });
for (const size of [16, 32, 128, 256, 512])
  for (const scale of [1, 2]) {
    renderJobs.push(
      raster(icon(size <= 32), size * scale).then((data) =>
        write(`Kosse.iconset/icon_${size}x${size}${scale === 2 ? "@2x" : ""}.png`, data),
      ),
    );
  }
await Promise.all(renderJobs);
let iconError;
try {
  execFileSync(
    "/usr/bin/iconutil",
    ["-c", "icns", root + "Kosse.iconset", "-o", root + "Kosse.icns"],
    { stdio: "pipe" },
  );
} catch (error) {
  iconError = error;
}
const tokens = {
  $description:
    "Kosse — Le raccord. Scheme groups contain W3C DTCG OKLCH colour tokens; hex preserves the accepted sRGB source.",
};
for (const [scheme, dark] of [
  ["light", false],
  ["dark", true],
]) {
  tokens[scheme] = {};
  for (const p of palette) {
    const hex = p[dark ? 5 : 3],
      components = p[dark ? 6 : 4];
    tokens[scheme][p[0]] = {
      $type: "color",
      $description: `${p[1]} — ${p[2]}.`,
      $value:
        p[0] === "danger" ? `{${scheme}.failed}` : { colorSpace: "oklch", components, alpha: 1 },
      $extensions: { "design.kosse": { hex, ...(p[0] === "danger" ? { oklch: components } : {}) } },
    };
  }
  for (const role of ["accent", "danger"])
    tokens[scheme][`on-${role}`] = {
      $type: "color",
      $description: `${dark ? "Fond" : "Plan"} — Text on solid ${role} buttons.`,
      $value: `{${scheme}.${dark ? "canvas" : "surface"}}`,
      $extensions: {
        "design.kosse": {
          hex: dark ? D.canvas : C.surface,
          oklch: palette.find((p) => p[0] === (dark ? "canvas" : "surface"))[dark ? 6 : 4],
        },
      },
    };
}
write("tokens.json", JSON.stringify(tokens, null, 2) + "\n");
const luminance = (hex) => {
  const v = hex
    .slice(1)
    .match(/../g)
    .map((c) => parseInt(c, 16) / 255)
    .map((channel) => (channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4));
  return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
};
const contrast = (a, b) => {
  const v = [luminance(a), luminance(b)].toSorted((first, second) => second - first);
  return (v[0] + 0.05) / (v[1] + 0.05);
};
const rows = palette
  .filter((p) => !["canvas", "surface", "focus", "unknown", "danger"].includes(p[0]))
  .map(
    (p) =>
      `| ${p[0]} · ${p[1]} | ${p[3]} / ${p[5]} | ${Math.min(contrast(p[3], C.canvas), contrast(p[3], C.surface)).toFixed(2)} | ${Math.min(contrast(p[5], D.canvas), contrast(p[5], D.surface)).toFixed(2)} |`,
  )
  .join("\n");
write(
  "BRAND.md",
  `# Kosse — Le raccord

## Idea

Kosse shows the development services running on your Mac and connects each endpoint to the process and project that own it. The name comes from Québécois “kossé ?” (“what's that?”), the question you ask about a port you don't recognise. That is etymology, not a tagline: Kosse has none. Write **Kosse** in prose and **kosse** in packages and the wordmark.

## Mark

One stem is the process; its two arms are the endpoints it owns. The short upper arm and longer, planted lower arm make an asymmetric K. Flat caps, open wedges, no nodes. This is ownership, not a Git history.

All geometry comes from [tools/mark.mjs](tools/mark.mjs), on a 16-unit grid: stem x=1–5, y=1–15; junction (4,7); arm endpoints (10.75,2.5) and (14,13.5); arm width 3. Small exports use 3.6. Never substitute a font K. Keep one stem-width of clear space in layouts; the asset canvas itself is tightly fitted. Minimum mark size: 16 px.

## Wordmark

Lowercase **kosse**, Bricolage Grotesque Bold 700, outlined and single-colour. Manual pair adjustments: ko −0.025 em, os −0.018 em, ss −0.012 em, se −0.018 em. The supplied static font determines its optical design; no variable optical-size claim. Use the assets rather than retyping. The lockup combines the mark with the unchanged wordmark.

## Colour

**Fleuve** is the St. Lawrence’s deep green-teal: the Québec link lives in colour, without symbols. Light Fleuve is #006B68; dark Fleuve is #71D6CD. Neutrals carry a slight green tint. Canvas: #F4F6F3 / #121A19; surface: #FFFFFF / #1C2725. Full OKLCH values and sRGB hex are in [tokens.json](tokens.json).

Contrast below is the lower WCAG ratio against canvas or surface, calculated from sRGB. Focus aliases Fleuve visually; unknown uses the muted-text values. Colour always accompanies state shapes and labels.

| Role · name | Light / dark hex | Light contrast | Dark contrast |
|---|---|---:|---:|
${rows}

Listening is an observation, not success. Reachable means an HTTP response, including errors, not health. Unknown stays readable. **Arrêt**, the danger token, aliases failed but names an action. Use red text in rows and a solid red button only for the final Stop confirmation.

Solid buttons use surface text in light mode and canvas text in dark mode. Accent: ${contrast(C.surface, C.accent).toFixed(2)}:1 / ${contrast(D.canvas, D.accent).toFixed(2)}:1; danger: ${contrast(C.surface, C.danger).toFixed(2)}:1 / ${contrast(D.canvas, D.danger).toFixed(2)}:1. Focus rings are 2 px with a 2 px background-coloured gap. Keep meaningful borders fully opaque.

## Type

Bricolage Grotesque is used only as outlined brand artwork. The dashboard uses system-ui for text and ui-monospace for ports, PIDs, paths and commands, with tabular figures for metrics. No bundled or requested web fonts.

## Tray

Only **default** and **attention**. The latter cuts a 2 × 2 pt notch from the lower stem at y=11, retaining 2 pt of stem. It means discovery failed or the snapshot is stale; an individual failed HTTP probe does not change the tray. The menu supplies the reason. Confirmation stays in the dialog.

The 18 pt template has black RGB and transparent alpha, a 14 pt tall glyph in a 16 pt inner grid, and whole-pixel stem and notch edges at 1×. macOS supplies the tint. No animation. The preview simulates tint on menu-bar greys; native template rendering remains to be checked in the app.

## Voice

Strictly plain, specific and unhurried, written for a developer looking after their own services. Never infer “forgotten”, “safe” or “healthy” from observations alone. No French in the interface; the name carries the Québec link.

- Empty after a successful scan: “No listeners found. Start a local service and it will appear here.”
- Stop confirmation, illustrative values: “Stop node (PID 4821) in storefront? This will request a graceful stop and affect ports :5173 and :5174.” Buttons: **Cancel** / **Stop process**.
- Unknown cwd: “Project unknown. The working directory isn’t available.”

## Assets

[README.md](README.md) lists the generated assets and regeneration command. The app icon is a white mark on a flat Fleuve superellipse, inset 100 px on its 1024 px canvas. The 16 and 32 pt iconset entries use heavier arms and a larger glyph. The favicon fills its canvas to preserve legibility. [preview.png](preview.png) shows the family at review sizes.
`,
);
// Review sheet uses actual exported PNGs for the tray and iconset, not approximations.
const width = 1600,
  height = 2100,
  layers = [];
const add = (input, left, top) => layers.push({ input, left, top });
const label = (text, left, top, size = 18, colour = C.ink) =>
  `<text x="${left}" y="${top}" font-family="Helvetica,Arial,sans-serif" font-size="${size}" fill="${colour}">${text}</text>`;
let sheet =
  `<rect width="1600" height="2100" fill="${C.canvas}"/>` +
  label("kosse / Le raccord", 48, 56, 32) +
  label("Marks · 16 / 18 / 32 / 64 / 256 px", 48, 100);
const previewJobs = [];
let mx = 48;
for (const size of [16, 18, 32, 64, 256]) {
  const left = mx;
  previewJobs.push(raster(mark(C.accent), size).then((data) => add(data, left, 130)));
  mx += size + 40;
}
sheet +=
  label("Lockups", 780, 100) +
  `<rect x="760" y="230" width="792" height="170" fill="${D.canvas}"/>`;
add(
  await sharp(Buffer.from(lockup(C.accent, C.ink)))
    .resize({ width: 500 })
    .png()
    .toBuffer(),
  790,
  115,
);
add(
  await sharp(Buffer.from(lockup(D.accent, D.ink)))
    .resize({ width: 500 })
    .png()
    .toBuffer(),
  790,
  265,
);
sheet += label("Tray · actual 18 px and 36 px exports · default / attention", 48, 445);
for (const [i, bg, fg] of [
  [0, "#E4E4E4", "#000000"],
  [1, "#303030", "#FFFFFF"],
]) {
  const top = 470 + i * 100;
  sheet +=
    `<rect x="48" y="${top}" width="1504" height="84" fill="${bg}"/>` +
    label("1×", 76, top + 48, 18, fg) +
    label("2× pixels", 450, top + 48, 18, fg);
  for (const [j, state] of ["default", "attention"].entries())
    for (const scale of [1, 2]) {
      const buf = readFileSync(root + `tray/tray-${state}${scale === 2 ? "@2x" : ""}.png`);
      const tinted = i
        ? sharp(buf).negate({ alpha: false }).png().toBuffer()
        : Promise.resolve(buf);
      previewJobs.push(
        tinted.then((data) =>
          add(data, (scale === 1 ? 180 : 610) + j * 130, top + Math.floor((84 - 18 * scale) / 2)),
        ),
      );
    }
}
sheet +=
  label("App icon · 1024 px master", 48, 710) +
  label("32 px optical icon", 1170, 760) +
  label("Favicon · 16 / 32 px", 1170, 880);
add(readFileSync(root + "app-icon-1024.png"), 48, 735);
add(readFileSync(root + "Kosse.iconset/icon_32x32.png"), 1170, 790);
add(await raster(icon(true, true), 16), 1170, 910);
add(readFileSync(root + "favicon-32.png"), 1220, 902);
sheet += label("Palette · light / dark", 48, 1790);
for (const [i, p] of palette.entries()) {
  const sx = 48 + (i % 7) * 216,
    sy = 1820 + Math.floor(i / 7) * 130;
  sheet +=
    `<rect x="${sx}" y="${sy}" width="92" height="48" fill="${p[3]}"/><rect x="${sx + 96}" y="${sy}" width="92" height="48" fill="${p[5]}"/>` +
    label(p[0] + " · " + p[1], sx, sy + 72, 15) +
    label(p[3] + " / " + p[5], sx, sy + 95, 13);
}
await Promise.all(previewJobs);
await sharp(Buffer.from(svg(`0 0 ${width} ${height}`, sheet)))
  .composite(layers)
  .png()
  .toFile(root + "preview.png");
const entries = [
  ...["kosse-mark.svg", "kosse-mark-night.svg", "kosse-mark-mono.svg"].map((n) => [
    n,
    "Parametric Le raccord; tools/mark.mjs",
  ]),
  ...["kosse-wordmark.svg", "kosse-wordmark-night.svg"].map((n) => [
    n,
    "Outlined Bricolage Grotesque Bold 700, OFL 1.1 (permits outlined artwork); manual kerning",
  ]),
  ...["kosse-lockup.svg", "kosse-lockup-night.svg"].map((n) => [
    n,
    "Le raccord + outlined Bricolage Grotesque Bold 700, OFL 1.1 (permits outlined artwork)",
  ]),
  ...["app-icon.svg", "app-icon-1024.png", "Kosse.icns", "favicon.svg", "favicon-32.png"].map(
    (n) => [n, "Le raccord on a generated Fleuve superellipse; tools/generate.mjs"],
  ),
  ...[16, 32, 128, 256, 512].flatMap((s) =>
    [1, 2].map((d) => [
      `Kosse.iconset/icon_${s}x${s}${d === 2 ? "@2x" : ""}.png`,
      "Generated icon; optical master at 16 and 32 pt",
    ]),
  ),
  ...["default", "attention"].flatMap((s) =>
    [".svg", ".png", "@2x.png"].map((e) => [
      `tray/tray-${s}${e}`,
      "Parametric monochrome Le raccord; tools/mark.mjs",
    ]),
  ),
  ["tokens.json", "Accepted Le raccord palette; tools/palette.mjs"],
  [
    "preview.png",
    "Sharp contact sheet of these assets; wordmark outlines from Bricolage Grotesque, OFL 1.1",
  ],
  ["BRAND.md", "Le raccord identity specification; tools/generate.mjs"],
  ["README.md", "Asset manifest and reproduction notes; tools/generate.mjs"],
  ...["mark.mjs", "palette.mjs", "generate.mjs"].map((n) => [
    "tools/" + n,
    "Original Kosse source tooling",
  ]),
];
write(
  "README.md",
  `# Kosse brand assets

All original Kosse artwork and tooling are MIT, under the repository licence. No font files are distributed. Wordmark outlines derive from Bricolage Grotesque by Mathieu Triay, SIL OFL 1.1; that licence permits outlined artwork without applying OFL to the resulting artwork. The geometry is independently authored; the generator structure and superellipse approach follow icitte’s brand tooling.

| Asset | Licence and source |
|---|---|
${entries.map(([n, s]) => `| [${n}](${n}) | MIT — ${s}. |`).join("\n")}

${iconError ? "**Packaging status:** ICNS is not present: /usr/bin/iconutil rejected the iconset as Invalid Iconset in this generation. All ten PNG entries remain available.\n" : ""}
## Regenerate

Requires Node.js, macOS /usr/bin/iconutil, and an external tool directory with opentype.js 1.3.4, @fontsource/bricolage-grotesque 5 and sharp 0.34 already installed. All three resolve through createRequire(BRAND_TOOLS); the repository needs no new dependency. The font file is files/bricolage-grotesque-latin-700-normal.woff.

\`\`\`sh
BRAND_TOOLS=/absolute/path/to/brand-tools node brand/tools/generate.mjs
\`\`\`

Run from the repository root. The generator writes only inside brand/, including this manifest and BRAND.md. Edit tools/mark.mjs for geometry, tools/palette.mjs for colour, and tools/generate.mjs for layout or copy; regenerate rather than editing outputs. The iconset is retained for inspection. If iconutil fails, the remaining assets still regenerate and the command exits nonzero; no substitute ICNS is fabricated. The repository’s existing oxfmt is applied when available. Preview tray colours simulate macOS template tint; verify native integration separately. Preview text uses the local Helvetica/Arial system face, so its rasterization may vary by macOS version.
`,
);
console.log(
  `Generated brand assets (ICNS status reported separately). Geometry: ${JSON.stringify(MARK)}; small: ${JSON.stringify(SMALL)}`,
);

const formatter = fileURLToPath(new URL("../../node_modules/.bin/oxfmt", import.meta.url));
if (existsSync(formatter)) execFileSync(formatter, ["--write", root], { stdio: "pipe" });
if (iconError) {
  console.error(
    "All other assets generated; iconutil failed:",
    iconError.stderr?.toString().trim(),
  );
  process.exitCode = 1;
}
