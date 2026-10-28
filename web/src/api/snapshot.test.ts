import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { service, snapshot } from "@/test/snapshots";

import { isSnapshot } from "./client";
import { advance, isNewer, offerSnapshot, snapshotQuery } from "./snapshot";

describe("snapshot ordering", () => {
  it("accepts only newer revisions of the same instance", () => {
    expect(isNewer(undefined, snapshot(1))).toBe(true);
    expect(isNewer(snapshot(4), snapshot(5))).toBe(true);
    expect(isNewer(snapshot(5), snapshot(5))).toBe(false);
    expect(isNewer(snapshot(5), snapshot(3))).toBe(false);
  });

  it("treats another instance as a restart, whatever its revision, unless it scanned earlier", () => {
    const restarted = { ...snapshot(1, "b"), scanner: { ...snapshot(41).scanner } };
    expect(isNewer(snapshot(40, "a"), restarted)).toBe(true);
    expect(isNewer(restarted, snapshot(40, "a"))).toBe(false);
  });

  it("keeps a snapshot an event delivered while the initial GET was in flight", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    let answer: ((value: Response) => void) | undefined;
    const response = new Promise<Response>((resolve) => {
      answer = resolve;
    });
    vi.stubGlobal("fetch", () => response);
    try {
      const loading = client.fetchQuery(snapshotQuery);
      offerSnapshot(client, snapshot(5));
      answer?.(new Response(JSON.stringify(snapshot(3))));
      expect((await loading).snapshot.revision).toBe(5);
    } finally {
      vi.unstubAllGlobals();
      client.clear();
    }
  });

  it("keeps the cached state for an out-of-order snapshot and tracks ended services", () => {
    const first = advance(undefined, snapshot(1, "a", [service(1), service(2)]));
    const second = advance(first, snapshot(2, "a", [service(1)]));
    expect(advance(second, snapshot(1, "a", [service(1), service(2)]))).toBe(second);
    expect(second.ended.map((entry) => entry.service.process.identity.pid)).toEqual([2]);
    expect(second.ended[0]?.lastSeenMs).toBe(3000);
  });

  it("rejects payloads of another schema", () => {
    expect(isSnapshot(snapshot(1))).toBe(true);
    expect(isSnapshot({ ...snapshot(1), schemaVersion: 3 })).toBe(false);
    expect(isSnapshot({ revision: 1 })).toBe(false);
  });
});
