import { describe, expect, it } from "vitest";

import { viewFor } from "./app";

describe("viewFor", () => {
  it("shows the dashboard at the root and at the served index file", () => {
    expect(viewFor("/")).toBe("dashboard");
    expect(viewFor("/index.html")).toBe("dashboard");
  });

  it("reports anything else as not found", () => {
    expect(viewFor("/services/demo")).toBe("not-found");
    expect(viewFor("/index.htm")).toBe("not-found");
    expect(viewFor("")).toBe("not-found");
  });
});
