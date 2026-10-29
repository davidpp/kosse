#!/usr/bin/env node
// Check the assets the app embeds, including alpha-only tray tint and font coverage.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import sharp from "sharp";
import { wordmark } from "./wordmark.mjs";
import { colours } from "./palette.mjs";
const root = fileURLToPath(new URL("../", import.meta.url));
const data = (path) => readFileSync(root + path);
const states = ["default", "attention", "preview", "preview-attention"];
await Promise.all(
  states.flatMap((state) =>
    [1, 2].map(async (scale) => {
      const path = `tray/tray-${state}${scale === 2 ? "@2x" : ""}.png`;
      const { data: pixels, info } = await sharp(data(path))
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true });
      assert.equal(info.width, 18 * scale, path);
      assert.equal(info.height, 18 * scale, path);
      assert.equal(info.channels, 4, path);
      let visible = 0,
        transparent = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        assert.equal(
          pixels[i] + pixels[i + 1] + pixels[i + 2],
          0,
          `${path}: template RGB must be black`,
        );
        if (pixels[i + 3] > 0) visible++;
        else transparent++;
      }
      assert(visible > 20 && transparent > 20, `${path}: opaque mark on transparent padding`);
    }),
  ),
);
const statePixels = await Promise.all(
  states.map((state) =>
    sharp(data(`tray/tray-${state}@2x.png`))
      .raw()
      .toBuffer(),
  ),
);
for (let i = 0; i < states.length; i++)
  for (let j = i + 1; j < states.length; j++) {
    assert(!statePixels[i].equals(statePixels[j]), `${states[i]} and ${states[j]} must differ`);
  }
const iconChecks = [];
for (const name of ["Kosse", "Kosse-preview"]) {
  assert.equal(data(`${name}.icns`).subarray(0, 4).toString(), "icns");
  for (const size of [16, 32, 128, 256, 512])
    for (const scale of [1, 2]) {
      const path = `${name}.iconset/icon_${size}x${size}${scale === 2 ? "@2x" : ""}.png`;
      iconChecks.push(
        sharp(data(path))
          .metadata()
          .then((meta) => {
            assert.equal(meta.width, size * scale, path);
            assert.equal(meta.height, size * scale, path);
            assert(meta.hasAlpha, path);
            assert(meta.exif?.length, `${path}: source provenance`);
          }),
      );
    }
}
await Promise.all(iconChecks);
assert(
  !data("app-icon-1024.png").equals(data("app-icon-preview-1024.png")),
  "preview identity must differ",
);
const bytes = data("fonts/KibernetykMono-SemiBold.ttf");
const { face, path, glyphs, positions } = wordmark(bytes);
for (const ch of "Kossé") assert(face.charToGlyphIndex(ch) > 0, `wordmark glyph ${ch}`);
const advances = [..."Kossé"].map((ch) => face.charToGlyph(ch).advanceWidth);
assert(
  advances.every((n) => n === advances[0]),
  "retain true monospace advances",
);
assert.equal(glyphs.length, 6, "Kossé shapes to five letters plus a combining acute");
assert.equal(positions.filter((p) => p.xAdvance > 0).length, 5, "five monospace letter advances");
assert(
  positions.some((p) => p.xAdvance === 0 && p.yOffset > 0),
  "acute is positioned over e",
);
assert(
  path.getBoundingBox().x2 > 700 && path.getBoundingBox().y1 < -170,
  "outlined wordmark includes final accented letter",
);
assert.match(data("fonts/OFL.txt").toString(), /SIL OPEN FONT LICENSE Version 1\.1/);
const luminance = (hex) => {
  const c = hex
    .slice(1)
    .match(/../g)
    .map((x) => parseInt(x, 16) / 255)
    .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
};
const contrast = (a, b) => {
  const x = [luminance(a), luminance(b)].toSorted((left, right) => right - left);
  return (x[0] + 0.05) / (x[1] + 0.05);
};
for (const dark of [false, true]) {
  const c = colours(dark);
  for (const ground of ["canvas", "surface"]) {
    for (const role of ["ink", "ink-muted", "accent", "danger"])
      assert(
        contrast(c[role], c[ground]) >= 4.5,
        `${dark ? "dark" : "light"} ${role}/${ground} text contrast`,
      );
    for (const role of ["line", "line-strong", "focus"])
      assert(
        contrast(c[role], c[ground]) >= 3,
        `${dark ? "dark" : "light"} ${role}/${ground} boundary contrast`,
      );
  }
}
console.log(
  "Brand assets verified: tray alpha/dimensions/states, both ICNS/iconsets, preview distinction, provenance, Kossé font coverage, monospace advances and palette contrast.",
);
