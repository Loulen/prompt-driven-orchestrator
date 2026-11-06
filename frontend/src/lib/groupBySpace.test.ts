import { describe, it, expect } from "vitest";
import { groupBySpace, NO_SPACE_KEY, sortSpaces, spaceLookup } from "./groupBySpace";
import type { Space } from "../types";

const loic: Space = { id: "spc-l", name: "Loïc", color: "blue" };
const ana: Space = { id: "spc-a", name: "ana", color: "orange" };
const bob: Space = { id: "spc-b", name: "Bob", color: "green" };

interface Item {
  id: string;
  space?: string | null;
}
const items: Item[] = [
  { id: "r1", space: "spc-l" },
  { id: "r2" },
  { id: "r3", space: "spc-a" },
  { id: "r4", space: "spc-l" },
  { id: "r5", space: "spc-gone" },
];

describe("groupBySpace (#1041)", () => {
  it("returns null when the instance has no Espace — the list stays as it was", () => {
    expect(groupBySpace(items, (i) => i.space, [])).toBeNull();
  });

  it("puts the Espaces first, by name, then « No space » last", () => {
    const groups = groupBySpace(items, (i) => i.space, [loic, bob, ana])!;
    expect(groups.map((g) => g.key)).toEqual([
      "space:spc-a",
      "space:spc-b",
      "space:spc-l",
      NO_SPACE_KEY,
    ]);
    expect(groups.map((g) => g.space?.name ?? null)).toEqual(["ana", "Bob", "Loïc", null]);
  });

  it("keeps an empty Espace (its header is a drop target) and input order inside", () => {
    const groups = groupBySpace(items, (i) => i.space, [loic, bob, ana])!;
    const byKey = new Map(groups.map((g) => [g.key, g.items.map((i) => i.id)]));
    expect(byKey.get("space:spc-b")).toEqual([]);
    expect(byKey.get("space:spc-l")).toEqual(["r1", "r4"]);
    expect(byKey.get("space:spc-a")).toEqual(["r3"]);
  });

  it("files an unknown Espace (deleted meanwhile) under « No space »", () => {
    const groups = groupBySpace(items, (i) => i.space, [loic, ana])!;
    expect(groups[groups.length - 1].items.map((i) => i.id)).toEqual(["r2", "r5"]);
  });

  it("sorts case-insensitively, ties broken by id", () => {
    const twin: Space = { id: "spc-0", name: "LOÏC", color: "sky" };
    expect(sortSpaces([loic, twin, ana]).map((s) => s.id)).toEqual(["spc-a", "spc-0", "spc-l"]);
  });

  it("looks an Espace up by id, null for none or unknown", () => {
    const lookup = spaceLookup([loic]);
    expect(lookup("spc-l")).toBe(loic);
    expect(lookup("spc-gone")).toBeNull();
    expect(lookup(null)).toBeNull();
    expect(lookup(undefined)).toBeNull();
  });
});
