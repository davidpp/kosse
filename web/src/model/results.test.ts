import { describe, expect, it } from "vitest";

import type { ActionResult } from "@/generated/contracts";

import { describeFailure, describeResult } from "./results";

const target = { pid: 42, startedAtSecs: 1_700_000_000 };

describe("action results", () => {
  it("calls only `exited` done", () => {
    const exited = describeResult({ outcome: "exited", target, ports: [5173], shared: [] }, "stop");
    expect(exited.tone).toBe("done");
    expect(exited.detail.join(" ")).toMatch(/PID 42 .*:5173 is free/);
    const notDone: ActionResult[] = [
      { outcome: "stillRunning", target, ports: [5173], forceUntilMs: 99 },
      { outcome: "respawned", target, parentPid: 7, owners: [], shared: [] },
      { outcome: "endpointTaken", target, owners: [], shared: [] },
      { outcome: "observationFailed", target, processExited: true, reason: "lsof failed" },
    ];
    for (const result of notDone) expect(describeResult(result, "stop").tone).not.toBe("done");
  });

  it("offers force only from stillRunning, with its deadline", () => {
    expect(
      describeResult({ outcome: "stillRunning", target, ports: [1], forceUntilMs: 99 }, "stop")
        .forceUntilMs,
    ).toBe(99);
    expect(
      describeResult({ outcome: "exited", target, ports: [1], shared: [] }, "stop").forceUntilMs,
    ).toBeNull();
  });

  it("says nothing was sent when the server refused before signalling", () => {
    expect(
      describeResult({ outcome: "stale", reason: "instanceChanged" }, "stop").detail,
    ).toContain("Kossé restarted after this view was loaded.");
    expect(
      describeResult({ outcome: "refused", reason: "outsideTestScope" }, "stop").detail,
    ).toContain("Nothing was sent.");
    const limited = describeFailure(
      {
        kind: "refused",
        status: 429,
        code: "rateLimited",
        message: "another stop is still being observed",
      },
      "stop",
    );
    expect(limited.tone).not.toBe("done");
    expect(limited.detail.join(" ")).toContain("Nothing was sent");
  });

  it("does not claim anything when the server did not answer", () => {
    expect(describeFailure({ kind: "unreachable", message: "x" }, "force").detail[0]).toContain(
      "may or may not have reached it",
    );
  });

  it("names co-owners that were not affected", () => {
    const shared = [
      { port: 5173, identity: { pid: 9, startedAtSecs: null }, name: "vite", parentPid: null },
    ];
    expect(
      describeResult({ outcome: "exited", target, ports: [5173], shared }, "stop").detail[1],
    ).toContain("vite (PID 9) on :5173");
  });
});
