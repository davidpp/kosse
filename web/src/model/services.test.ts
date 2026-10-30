import { describe, expect, it } from "vitest";

import type { ProjectMetadata, Service } from "@/generated/contracts";
import { endpoint, observation, service } from "@/test/snapshots";

import { buildDashboard, ENDED_RETENTION_MS, isNoise, searchTerms, trackEnded } from "./services";

const all = { terms: [], allListeners: true };
const projects = { terms: [], allListeners: false };

describe("noise", () => {
  it("hides listeners outside a project and other users' processes by default", () => {
    const outside = service(1, { project: null, name: "rapportd" });
    const other = service(2, {
      endpoints: [endpoint(5432, { state: "skipped", reason: "otherUser" })],
    });
    const mine = service(3);
    expect([outside, other, mine].map(isNoise)).toEqual([true, true, false]);

    const view = buildDashboard(observation([outside, other, mine]), [], projects);
    expect(view.hidden).toBe(2);
    expect(view.groups.flatMap((group) => group.rows.map((row) => row.key))).toEqual([
      `service:3.${1_700_000_003}`,
    ]);
  });

  it("shows everything with all listeners, the unattributed in their own group last", () => {
    const view = buildDashboard(observation([service(1, { project: null }), service(2)]), [], all);
    expect(view.hidden).toBe(0);
    expect(view.groups.map((group) => group.title)).toEqual(["shop", "No project"]);
  });
});

function searchRows(services: Service[], query: string) {
  return buildDashboard(observation(services), [], {
    terms: searchTerms(query),
    allListeners: false,
  }).groups.flatMap((group) => group.rows);
}

describe("search", () => {
  const services = [
    service(1, {
      name: "vite",
      ports: [5173],
      package: { path: "apps/web", name: "web", manifests: ["package.json"] },
    }),
    service(2, { name: "postgres", ports: [5432], project: null }),
  ];

  it("matches project, package, command and port, every term at once", () => {
    expect(searchRows(services, ":5173")).toHaveLength(1);
    expect(searchRows(services, "SHOP web")).toHaveLength(1);
    expect(searchRows(services, "--dev")).toHaveLength(1);
    expect(searchRows(services, "vite :9999")).toHaveLength(0);
  });

  it("counts hidden matches so the view can offer all listeners", () => {
    const view = buildDashboard(observation(services), [], {
      terms: searchTerms("5432"),
      allListeners: false,
    });
    expect(view.groups).toHaveLength(0);
    expect(view.hiddenMatches).toBe(1);
  });
});

describe("profiles", () => {
  const metadata: ProjectMetadata = {
    root: "/work/shop",
    observed: true,
    sources: ["central"],
    name: "Shop",
    nameSource: "central",
    services: [
      {
        id: "api",
        name: "Shop API",
        match: { cwd: "apps/api" },
        protocol: "http",
        probePath: null,
        endpoints: [],
        otherEndpoints: null,
        links: [],
        disabled: false,
        sources: {
          name: "central",
          match: "central",
          protocol: "central",
          probePath: null,
          endpoints: null,
          otherEndpoints: null,
          links: null,
          disabled: null,
        },
        matched: [{ pid: 1, startedAtSecs: 1_700_000_001 }],
      },
      {
        id: "worker",
        name: null,
        match: { port: 7000 },
        protocol: null,
        probePath: null,
        endpoints: [],
        otherEndpoints: null,
        links: [],
        disabled: false,
        sources: {
          name: null,
          match: "repository",
          protocol: null,
          probePath: null,
          endpoints: null,
          otherEndpoints: null,
          links: null,
          disabled: null,
        },
        matched: [],
      },
    ],
  };

  it("names matched services with provenance and lists configured, not observed entries", () => {
    const scan = {
      ...observation([service(1, { profile: { kind: "matched", service: "api" } })]),
      profiles: [metadata],
    };
    const [group] = buildDashboard(scan, [], projects).groups;
    expect(group?.title).toBe("Shop");
    expect(group?.titleSource).toBe("central");
    const [first, second] = group?.rows ?? [];
    expect(first?.kind === "service" && [first.name, first.nameSource]).toEqual([
      "Shop API",
      "central",
    ]);
    expect(second?.kind).toBe("configured");
  });
});

describe("ended services", () => {
  it("keeps a vanished service with its last-seen scan time, for the retention window only", () => {
    const before = observation([service(1), service(2)], 1000);
    const after = observation([service(1)], 4000);
    const ended = trackEnded([], before, after);
    expect(ended.map((entry) => [entry.service.process.identity.pid, entry.lastSeenMs])).toEqual([
      [2, 1000],
    ]);

    const view = buildDashboard(after, ended, projects);
    expect(view.groups[0]?.rows.map((row) => row.kind)).toEqual(["service", "ended"]);

    expect(
      trackEnded(ended, after, observation([service(1)], 1000 + ENDED_RETENTION_MS - 1)),
    ).toHaveLength(1);
    expect(
      trackEnded(ended, after, observation([service(1)], 1000 + ENDED_RETENTION_MS)),
    ).toHaveLength(0);
  });
});
