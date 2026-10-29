// Product colours, not an official flag colour specification. All are in sRGB.
export const palette = [
  ["ink", "Ink", "Primary text", "#232326", "#EDEDEB"],
  ["ink-muted", "Graphite", "Secondary text", "#606069", "#AAAAB4"],
  ["canvas", "Canvas", "Window background", "#F7F7F5", "#18181A"],
  ["surface", "Surface", "Raised controls and inspector", "#FFFFFF", "#232326"],
  ["line", "Line", "Meaningful boundary", "#8C8C96", "#777782"],
  ["line-strong", "Edge", "Control boundary", "#73737D", "#9696A2"],
  ["accent", "Québec blue", "Selection and primary action", "#003DA5", "#91B3FF"],
  ["focus", "Focus", "Keyboard focus", "#003DA5", "#91B3FF"],
  ["listening", "Listening", "Observed TCP listener; not success", "#555560", "#BCBCC6"],
  [
    "reachable",
    "Reachable",
    "HTTP response received, including errors; not health",
    "#255DC8",
    "#9BBCFF",
  ],
  ["stale", "Stale", "Snapshot is stale", "#805700", "#E4BA69"],
  ["failed", "Failed", "Observed failure", "#AF3041", "#FF9AAB"],
  ["unknown", "Unknown", "Unavailable observation, still readable", "#606069", "#AAAAB4"],
  ["danger", "Stop", "Destructive action; aliases failed", "#AF3041", "#FF9AAB"],
];
export const colours = (dark = false) =>
  Object.fromEntries(palette.map((p) => [p[0], p[dark ? 4 : 3]]));
export function oklch(hex) {
  const [r, g, b] = hex
    .slice(1)
    .match(/../g)
    .map((v) => parseInt(v, 16) / 255)
    .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b),
    m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b),
    s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [L, Math.hypot(a, bb), ((Math.atan2(bb, a) * 180) / Math.PI + 360) % 360].map(
    (v) => +v.toFixed(4),
  );
}
