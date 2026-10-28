import { describe, expect, it } from "vitest";

import { parseRoute, routeHref } from "./route";

describe("routes", () => {
  it("reads the dashboard, inspect and stop deep links", () => {
    expect(parseRoute("/", "")).toEqual({ view: "dashboard" });
    expect(parseRoute("/index.html", "?inspect=4821.1700000000")).toEqual({
      view: "inspect",
      identity: "4821.1700000000",
    });
    expect(parseRoute("/", "?stop=4821.1700000000")).toEqual({
      view: "stop",
      identity: "4821.1700000000",
    });
    expect(routeHref({ view: "stop", identity: "1.2" })).toBe("/?stop=1.2");
  });

  it("ignores malformed identities and reports other paths as not found", () => {
    expect(parseRoute("/", "?stop=4821")).toEqual({ view: "dashboard" });
    expect(parseRoute("/", "?inspect=<script>")).toEqual({ view: "dashboard" });
    expect(parseRoute("/services", "")).toEqual({ view: "not-found" });
  });
});
