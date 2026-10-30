import { describe, expect, it } from "vitest";

import { endpoint, service } from "@/test/snapshots";

import { displayReason, splitEndpoints } from "./endpoints";

const answered = {
  state: "probed",
  probedAtMs: 1,
  requested: false,
  result: {
    kind: "response",
    url: "http://localhost:9229/",
    status: 404,
    title: null,
    location: null,
    tls: false,
    bodyTruncated: false,
  },
} as const;

describe("endpoint tiers", () => {
  it("offers only summary endpoints as launch targets, whatever answered HTTP", () => {
    const vite = service(1, {
      endpoints: [
        endpoint(3000, answered, {
          name: "Site",
          show: "summary",
          basis: "endpoint",
          source: "central",
        }),
        endpoint(9229, answered, {
          name: "Inspector",
          show: "details",
          basis: "inspectorPort",
          source: null,
        }),
        endpoint(40123, answered, {
          name: null,
          show: "details",
          basis: "otherEndpoints",
          source: "central",
        }),
      ],
    });
    const { summary, details } = splitEndpoints(vite);
    expect(summary.map((each) => each.port)).toEqual([3000]);
    expect(details.map((each) => each.port)).toEqual([9229, 40123]);

    const workerd = service(2, {
      endpoints: [
        endpoint(51001, answered, {
          name: null,
          show: "details",
          basis: "otherEndpoints",
          source: "central",
        }),
      ],
    });
    expect(splitEndpoints(workerd).summary).toEqual([]);
  });

  it("says where each tier came from", () => {
    expect(
      displayReason({ name: "Site", show: "summary", basis: "endpoint", source: "repository" }),
    ).toBe("kosse.json: endpoints");
    expect(
      displayReason({ name: null, show: "details", basis: "otherEndpoints", source: "central" }),
    ).toBe("Kossé profile: other endpoints");
    expect(
      displayReason({ name: "Inspector", show: "details", basis: "inspectorPort", source: null }),
    ).toBe("inferred: default inspector port");
  });
});
