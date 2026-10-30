import { describe, expect, it } from "vitest";

import type { ConfiguredService, ProjectMetadata } from "@/generated/contracts";

import { buildProfile, initialDrafts } from "./customize-dialog";

const draft = {
  uid: 1,
  id: "api",
  name: "",
  cwd: "",
  port: "",
  process: "",
  protocol: "" as const,
  probePath: "",
  endpoints: [],
  endpointsSet: false,
  otherEndpoints: "" as const,
  links: [],
  linksSet: false,
  disabled: false,
  effective: null,
};

describe("customize form", () => {
  it("builds the central profile from filled fields only", () => {
    expect(
      buildProfile(" Shop ", [
        {
          ...draft,
          name: "API",
          cwd: "apps/api",
          port: "3000",
          protocol: "http",
          probePath: "/health",
          links: [{ uid: 2, label: "Docs", url: "https://docs.shop.test" }],
        },
      ]),
    ).toEqual({
      version: 1,
      name: "Shop",
      services: {
        api: {
          name: "API",
          match: { cwd: "apps/api", port: 3000 },
          protocol: "http",
          probePath: "/health",
          links: [{ label: "Docs", url: "https://docs.shop.test" }],
        },
      },
    });
  });

  it("round-trips a central suppression, an explicit empty link list and link kinds", () => {
    expect(
      buildProfile("", [
        { ...draft, id: "legacy", disabled: true },
        { ...draft, id: "web", linksSet: true },
        {
          ...draft,
          id: "tunnel",
          linksSet: true,
          links: [{ uid: 4, label: "Share", url: "https://shop.example", kind: "tunnel" }],
        },
      ]),
    ).toEqual({
      version: 1,
      services: {
        legacy: { disabled: true },
        web: { links: [] },
        tunnel: { links: [{ label: "Share", url: "https://shop.example", kind: "tunnel" }] },
      },
    });
  });

  it("saves back what the central profile set, untouched, and nothing from kosse.json", () => {
    const none = {
      name: null,
      match: null,
      protocol: null,
      probePath: null,
      endpoints: null,
      otherEndpoints: null,
      links: null,
      disabled: null,
    } as const;
    const entry = (id: string, fields: Partial<ConfiguredService>): ConfiguredService => ({
      id,
      name: null,
      match: null,
      protocol: null,
      probePath: null,
      endpoints: [],
      otherEndpoints: null,
      links: [],
      disabled: false,
      sources: none,
      matched: [],
      ...fields,
    });
    const metadata: ProjectMetadata = {
      root: "/work/shop",
      observed: true,
      sources: ["central", "repository"],
      name: "Shop",
      nameSource: "central",
      centralRevision: "0123456789abcdef",
      services: [
        entry("legacy", { disabled: true, sources: { ...none, disabled: "central" } }),
        entry("web", {
          name: "Web",
          match: { cwd: "apps/web" },
          sources: { ...none, name: "repository", match: "repository", links: "central" },
        }),
        entry("api", {
          name: "API",
          probePath: "/health",
          links: [
            {
              label: "Docs",
              url: "https://docs.shop.test/",
              host: "docs.shop.test",
              kind: "external",
              reach: "declared",
            },
          ],
          sources: { ...none, name: "central", probePath: "repository", links: "central" },
        }),
        entry("repo-only", { name: "From repo", sources: { ...none, name: "repository" } }),
        entry("worker", {
          match: { cwd: ".", process: "workerd" },
          endpoints: [
            { port: 3000, name: "Site", show: null, probePath: "/health" },
            { port: 9229, name: null, show: "details", probePath: null },
          ],
          otherEndpoints: "details",
          sources: { ...none, match: "central", endpoints: "central", otherEndpoints: "central" },
        }),
        entry("repo-endpoints", {
          endpoints: [{ port: 4000, name: "Admin", show: null, probePath: null }],
          sources: { ...none, endpoints: "repository", name: "central" },
        }),
      ],
    };
    expect(buildProfile("Shop", initialDrafts(metadata))).toEqual({
      version: 1,
      name: "Shop",
      services: {
        legacy: { disabled: true },
        web: { links: [] },
        api: {
          name: "API",
          links: [{ label: "Docs", url: "https://docs.shop.test/", kind: "external" }],
        },
        worker: {
          match: { cwd: ".", process: "workerd" },
          endpoints: {
            "3000": { name: "Site", probePath: "/health" },
            "9229": { show: "details" },
          },
          otherEndpoints: { show: "details" },
        },
        "repo-endpoints": {},
      },
    });
  });

  it("builds endpoints keyed by port, a bare port meaning summary", () => {
    const endpoint = { uid: 9, port: "3000", name: "", show: "" as const, probePath: "" };
    expect(
      buildProfile("", [
        { ...draft, endpoints: [endpoint, { ...endpoint, port: "" }], otherEndpoints: "details" },
      ]),
    ).toEqual({
      version: 1,
      services: {
        api: { endpoints: { "3000": { show: "summary" } }, otherEndpoints: { show: "details" } },
      },
    });
    expect(buildProfile("", [{ ...draft, endpointsSet: true }])).toEqual({
      version: 1,
      services: { api: { endpoints: {} } },
    });
    expect(buildProfile("", [{ ...draft, endpoints: [endpoint, endpoint] }])).toBe(
      "api: endpoint :3000 is listed twice",
    );
  });

  it("removes the profile when nothing is set", () => {
    expect(buildProfile("", [])).toBeNull();
  });

  it("names the first invalid field", () => {
    expect(buildProfile("", [{ ...draft, port: "http" }])).toBe("api: the port must be a number");
    expect(buildProfile("", [{ ...draft, probePath: "health" }])).toBe(
      "api: the probe path must start with /",
    );
    expect(
      buildProfile("", [{ ...draft, links: [{ uid: 3, label: "x", url: "javascript:alert(1)" }] }]),
    ).toBe('api: "javascript:alert(1)" is not an http or https URL');
    expect(buildProfile("", [draft, draft])).toBe('the service ID "api" is used twice');
  });
});
