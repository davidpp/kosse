import { describe, expect, it } from "vitest";

import { draftView } from "./draft";

const a = { revision: "a", profile: "agent before" };
const b = { revision: "b", profile: "agent after" };

describe("draft reconciliation", () => {
  it("an untouched draft re-renders from another writer's profile", () => {
    expect(draftView({ live: b, pinned: null, conflicted: false })).toEqual({
      shown: b,
      changedOutside: null,
    });
  });

  it("an edited draft is never replaced silently: it stays, the change is shown, reload offered", () => {
    expect(draftView({ live: b, pinned: a, conflicted: false })).toEqual({
      shown: a,
      changedOutside: { reloadable: true },
    });
  });

  it("a refused save shows the same state, reloadable once the snapshot has the change", () => {
    expect(draftView({ live: a, pinned: a, conflicted: true })).toEqual({
      shown: a,
      changedOutside: { reloadable: false },
    });
    expect(draftView({ live: b, pinned: a, conflicted: true }).changedOutside).toEqual({
      reloadable: true,
    });
  });

  it("an edited draft is left alone while nobody else wrote", () => {
    expect(draftView({ live: a, pinned: a, conflicted: false })).toEqual({
      shown: a,
      changedOutside: null,
    });
  });
});
