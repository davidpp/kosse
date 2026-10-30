import { describe, expect, it } from "vitest";

import { safeLink, sameLink } from "./links";

describe("safeLink", () => {
  it("keeps http and https URLs and exposes their host", () => {
    expect(safeLink("http://localhost:5173/")).toEqual({
      href: "http://localhost:5173/",
      host: "localhost:5173",
      hostname: "localhost",
    });
    expect(safeLink("https://Shop.Localhost/docs")).toEqual({
      href: "https://shop.localhost/docs",
      host: "shop.localhost",
      hostname: "shop.localhost",
    });
  });

  it.each([
    "javascript:alert(1)",
    " JavaScript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "file:///etc/passwd",
    "vscode://open",
    "https://user:pass@example.com/",
    "//example.com",
    "/relative",
    "",
  ])("refuses %j (other schemes, credentials, relative URLs)", (url) => {
    expect(safeLink(url)).toBeNull();
  });
});

describe("sameLink", () => {
  it("matches a declared link to the probed URL it duplicates, and nothing else", () => {
    expect(sameLink("http://localhost:3000", "http://localhost:3000/")).toBe(true);
    expect(sameLink("http://LOCALHOST:3000/", "http://localhost:3000/")).toBe(true);
    expect(sameLink("http://localhost:3000/docs", "http://localhost:3000/")).toBe(false);
    expect(sameLink("http://127.0.0.1:3000/", "http://localhost:3000/")).toBe(false);
    expect(sameLink("javascript:alert(1)", "javascript:alert(1)")).toBe(false);
  });
});
