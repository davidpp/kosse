import { describe, expect, it } from "vitest";

import { endpoint, service } from "@/test/snapshots";

import type { HttpProbe } from "@/generated/contracts";

import { displayReason, openTarget, splitEndpoints } from "./endpoints";
import { summarizeProbe } from "./probe";

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

function response(port: number, status: number, title: string | null): HttpProbe {
  return {
    ...answered,
    result: { ...answered.result, url: `http://localhost:${port}/`, status, title },
  };
}

const summaryDisplay = { name: null, show: "summary", basis: "default", source: null } as const;
const item = (port: number, http: HttpProbe) => ({
  endpoint: endpoint(port, http, summaryDisplay),
  http,
});

describe("open target", () => {
  it("leads with the endpoint that served a page, then a non-error status, then the lowest port", () => {
    // The owner's Wrangler stack: a 404 on a low port must not outrank the page on :3000.
    const page = item(3000, response(3000, 200, "Site"));
    expect(openTarget([item(2999, response(2999, 404, null)), page])).toBe(page);
    const ok = item(60560, response(60560, 200, null));
    expect(openTarget([item(60559, response(60559, 500, null)), ok])).toBe(ok);
    const low = item(3000, response(3000, 404, null));
    expect(openTarget([low, item(3001, response(3001, 404, null))])).toBe(low);
  });

  it("is nothing when no summary endpoint answered HTTP", () => {
    const closed: HttpProbe = {
      ...answered,
      result: { kind: "closed" },
    };
    expect(openTarget([item(60561, closed), item(5432, { state: "pending" })])).toBeNull();
  });
});

describe("not-HTTP cue", () => {
  it("rules HTTP out only on a known non-HTTP listener or a non-HTTP answer", () => {
    expect(summarizeProbe({ state: "skipped", reason: "knownNonHttpPort" }).notHttp).toBe(true);
    expect(summarizeProbe({ state: "skipped", reason: "knownNonHttpProcess" }).notHttp).toBe(true);
    expect(summarizeProbe({ ...answered, result: { kind: "notHttp" } }).notHttp).toBe(true);
    // Unknown stays unknown: unchecked, failed or merely unprobed listeners are not called TCP.
    expect(summarizeProbe({ state: "pending" }).notHttp).toBe(false);
    expect(summarizeProbe({ ...answered, result: { kind: "closed" } }).notHttp).toBe(false);
    expect(summarizeProbe({ state: "skipped", reason: "notAttributed" }).notHttp).toBe(false);
    expect(summarizeProbe(answered).notHttp).toBe(false);
  });

  it("keeps the page title off the brief line only", () => {
    const seen = summarizeProbe(response(3000, 200, "Site"));
    expect(seen.text).toBe("HTTP 200 · Site");
    expect(seen.brief).toBe("HTTP 200");
  });
});
