// Enforces the web performance budget (blueprint/architecture.md): every JS and CSS file the
// binary ships, gzipped one by one, must total at most 50 kB, and the bundle carries no web fonts.
// Runs against an existing `web/dist`, so `test` and `build` call it right after `vite build`.
import { readdir } from "node:fs/promises";
import { extname, join } from "node:path";
import { gzipSync } from "node:zlib";

// Decimal kilobytes, the unit Vite's build report prints.
const BUDGET_BYTES = 50 * 1000;
const COUNTED = new Set([".js", ".mjs", ".css"]);
const FONTS = new Set([".woff", ".woff2", ".ttf", ".otf", ".eot"]);

// Paths in messages are relative to the repository root, the directory `bun run` uses.
const distPath = "web/dist";
const dist = join(import.meta.dir, "..", distPath);

let entries: string[];
try {
  entries = await readdir(dist, { recursive: true });
} catch {
  fail(`${distPath} is missing; run \`bun run --cwd web build\` first`);
}

const fonts = entries.filter((path) => FONTS.has(extname(path)));
if (fonts.length > 0) {
  fail(`web fonts are not allowed in the bundle (system font stack only): ${fonts.join(", ")}`);
}

const assets = await Promise.all(
  entries
    .filter((path) => COUNTED.has(extname(path)))
    .toSorted()
    .map(async (path) => {
      const bytes = await Bun.file(join(dist, path)).bytes();
      return { path, bytes, raw: bytes.byteLength, gzip: gzipSync(bytes).byteLength };
    }),
);
if (assets.length === 0) {
  fail(`no JS or CSS found under ${distPath}; the build looks incomplete`);
}

// A font inlined as a data: URI arrives through CSS rather than as a font file.
const inlinedFonts = assets.filter(
  (asset) =>
    extname(asset.path) === ".css" && new TextDecoder().decode(asset.bytes).includes("@font-face"),
);
if (inlinedFonts.length > 0) {
  fail(
    `web fonts are not allowed in the bundle: @font-face in ${inlinedFonts.map((asset) => asset.path).join(", ")}`,
  );
}

const total = assets.reduce((sum, asset) => sum + asset.gzip, 0);
for (const asset of assets) {
  console.log(
    `${kB(asset.gzip).padStart(9)} gzip  ${kB(asset.raw).padStart(9)} raw  ${asset.path}`,
  );
}
const summary = `${kB(total)} of ${kB(BUDGET_BYTES)} gzipped JS + CSS`;
if (total > BUDGET_BYTES) {
  fail(`${summary}, over the budget by ${kB(total - BUDGET_BYTES)}`);
}
console.log(`bundle-budget: ${summary} (${kB(BUDGET_BYTES - total)} headroom)`);

function kB(bytes: number): string {
  return `${(bytes / 1000).toFixed(2)} kB`;
}

function fail(message: string): never {
  console.error(`bundle-budget: ${message}`);
  process.exit(1);
}
