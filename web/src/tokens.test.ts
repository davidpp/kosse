// web/src/styles.css mirrors brand/tokens.json (the brand's DTCG source); this keeps them equal.
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

const root = join(import.meta.dirname, "..", "..");

type Token = { $value: { components: number[] } | string };

function brand(scheme: "light" | "dark"): Record<string, string> {
  const tokens: Record<string, Record<string, Token>> = JSON.parse(
    readFileSync(join(root, "brand", "tokens.json"), "utf8"),
  );
  const resolve = (token: Token | undefined): string => {
    if (token === undefined) throw new Error("dangling token reference");
    if (typeof token.$value === "string") {
      const [group, name] = token.$value.replace(/[{}]/g, "").split(".");
      return resolve(tokens[group ?? ""]?.[name ?? ""]);
    }
    return `oklch(${token.$value.components.join(" ")})`;
  };
  return Object.fromEntries(
    Object.entries(tokens[scheme] ?? {}).map(([name, token]) => [name, resolve(token)]),
  );
}

function css(block: "light" | "dark"): Record<string, string> {
  const source = readFileSync(join(root, "web", "src", "styles.css"), "utf8");
  const start =
    block === "light"
      ? source.indexOf("@theme static")
      : source.indexOf("@media (prefers-color-scheme: dark)");
  const section = source.slice(start, source.indexOf("}", start));
  return Object.fromEntries(
    [...section.matchAll(/--color-([a-z-]+): (oklch\([^)]*\));/g)].map((match) => [
      match[1],
      match[2],
    ]),
  );
}

describe("brand tokens", () => {
  it.each(["light", "dark"] as const)("%s roles match brand/tokens.json", (scheme) => {
    expect(css(scheme)).toEqual(brand(scheme));
  });
});
