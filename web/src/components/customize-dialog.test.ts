import { describe, expect, it } from "vitest";

import type { ConfiguredService, ProjectMetadata } from "@/generated/contracts";

import { buildProfile, initialDrafts } from "./customize-dialog";

const draft = {
  uid: 1,
  id: "api",
  name: "",
  cwd: "",
  port: "",
  protocol: "" as const,
  probePath: "",
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
      links: null,
      disabled: null,
    } as const;
    const entry = (id: string, fields: Partial<ConfiguredService>): ConfiguredService => ({
      id,
      name: null,
      match: null,
      protocol: null,
      probePath: null,
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
      },
    });
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
