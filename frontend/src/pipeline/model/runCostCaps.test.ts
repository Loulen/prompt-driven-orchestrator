import { describe, expect, it } from "vitest";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import { costCapEdits, foldCostCapOverrides } from "./runCostCaps";

function region(id: string, max_cost_usd: LoopRegion["max_cost_usd"], kind: LoopRegion["kind"] = "bounded"): LoopRegion {
  return kind === "bounded"
    ? { id, kind, members: ["a"], max_iter: 5, max_cost_usd }
    : { id, kind, members: ["a"], over: "items" };
}

describe("foldCostCapOverrides", () => {
  it("puts the Run's live overrides in place of the declared caps, null as no cap", () => {
    const pipeline = { loops: [region("r1", 5), region("r2", "$budget"), region("r3", 2)] };
    const folded = foldCostCapOverrides(pipeline, { r1: 8, r2: null });
    expect(folded.loops.map((r) => r.max_cost_usd)).toEqual([8, null, 2]);
    expect(pipeline.loops[0].max_cost_usd).toBe(5);
  });

  it("returns the pipeline itself when nothing applies, and never caps a collection", () => {
    const pipeline = { loops: [region("r1", 5), region("c", null, "collection")] };
    expect(foldCostCapOverrides(pipeline, undefined)).toBe(pipeline);
    expect(foldCostCapOverrides(pipeline, {})).toBe(pipeline);
    expect(foldCostCapOverrides(pipeline, { c: 3 })).toBe(pipeline);
  });
});

describe("costCapEdits", () => {
  it("issues one edit per bounded loop whose cap changed: raised, lowered, removed, added", () => {
    const before = { loops: [region("up", 5), region("down", 5), region("gone", 5), region("new", null), region("same", 5)] };
    const after = { loops: [region("up", 8), region("down", 4), region("gone", null), region("new", 3), region("same", 5)] };
    expect(costCapEdits(before, after)).toEqual([
      { regionId: "up", usd: 8 },
      { regionId: "down", usd: 4 },
      { regionId: "gone", usd: null },
      { regionId: "new", usd: 3 },
    ]);
  });

  it("leaves a $var cap to the YAML and never touches a collection", () => {
    const before = { loops: [region("v", 5), region("c", null, "collection")] };
    const after = { loops: [region("v", "$budget"), region("c", null, "collection")] };
    expect(costCapEdits(before, after)).toEqual([]);
  });

  it("compares against the Run's applied cap, so an untouched override issues nothing", () => {
    const snapshot = { loops: [region("r", 5)] };
    const applied = foldCostCapOverrides(snapshot, { r: 4 });
    expect(costCapEdits(applied, applied)).toEqual([]);
    expect(costCapEdits(applied, { loops: [region("r", 8)] })).toEqual([{ regionId: "r", usd: 8 }]);
  });
});
