import { describe, expect, it } from "vitest";

import { safeLink } from "./links";

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
