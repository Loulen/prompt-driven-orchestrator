import { describe, expect, it } from "vitest";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import {
  costCapBaseline,
  costCapEdits,
  foldCostCapOverrides,
  followCostCaps,
  runAppliedCaps,
  savedCostCapBaseline,
} from "./runCostCaps";

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
  const took = (yaml: LoopRegion["max_cost_usd"], usd: number | null = typeof yaml === "number" ? yaml : null) => ({ usd, yaml });

  it("issues one edit per bounded loop the user changed: raised, lowered, removed, added", () => {
    const baseline = { up: took(5), down: took(5), gone: took(5), new: took(null), same: took(5) };
    const saved = { loops: [region("up", 8), region("down", 4), region("gone", null), region("new", 3), region("same", 5)] };
    const applied = { up: 5, down: 5, gone: 5, new: null, same: 5 };
    expect(costCapEdits(saved, baseline, applied)).toEqual([
      { regionId: "up", usd: 8 },
      { regionId: "down", usd: 4 },
      { regionId: "gone", usd: null },
      { regionId: "new", usd: 3 },
    ]);
  });

  it("leaves a $var cap to the YAML and never touches a collection", () => {
    const saved = { loops: [region("v", "$budget"), region("c", null, "collection")] };
    expect(costCapEdits(saved, { v: took(5) }, { v: 5 })).toEqual([]);
  });

  it("leaves an untouched loop alone, even when the Run's cap moved since the tab took it", () => {
    // `--max-cost 4` from the CLI, the tab not caught up yet: a Save keeps 4.
    const saved = { loops: [region("r", 8), region("v", "$budget")] };
    expect(costCapEdits(saved, { r: took(8), v: took("$budget", 5) }, { r: 4, v: 5 })).toEqual([]);
  });

  it("sends nothing for a change that matches what the Run applies, and compares to it without a baseline", () => {
    const saved = { loops: [region("r", 4)] };
    expect(costCapEdits(saved, { r: took(8) }, { r: 4 })).toEqual([]);
    expect(costCapEdits({ loops: [region("r", 8)] }, undefined, { r: 4 })).toEqual([{ regionId: "r", usd: 8 }]);
  });
});

describe("savedCostCapBaseline", () => {
  it("takes the saved caps as the Run's, and leaves a refused one the user's", () => {
    const saved = { loops: [region("a", 8), region("b", 9), region("c", "$budget")] };
    const baseline = savedCostCapBaseline(saved, { a: 5, b: 5, c: 2 }, [{ regionId: "a", usd: 8 }], ["b"]);
    expect(baseline).toEqual({ a: { usd: 8, yaml: 8 }, c: { usd: 2, yaml: "$budget" } });
  });

  it("keeps what the tab took before for a refused loop, so undoing the change makes it untouched again", () => {
    const saved = { loops: [region("b", 12)] };
    const previous = { b: { usd: 8, yaml: 8 } };
    expect(savedCostCapBaseline(saved, { b: 8 }, [], ["b"], previous)).toEqual(previous);
  });
});

describe("runAppliedCaps", () => {
  const pipeline = {
    loops: [
      region("over", 5),
      region("served", "$budget"),
      region("var", "$budget"),
      region("unresolved", "$nope"),
      region("lit", 3),
      region("none", null),
      region("c", null, "collection"),
    ],
    variables: { budget: { default: 2 } },
  };

  it("reads the override, then the served cap, then the YAML literal or the $var's value", () => {
    const run = {
      region_max_cost_overrides: { over: 8 },
      loop_costs: { served: { cap_usd: 6 } },
      variables: { budget: 5 },
    };
    expect(runAppliedCaps(pipeline, run)).toEqual({ over: 8, served: 6, var: 5, lit: 3, none: null });
  });

  it("falls back on the declared default off a Run, and keeps a removed cap as null", () => {
    expect(runAppliedCaps(pipeline, { region_max_cost_overrides: { lit: null } })).toEqual({
      over: 5,
      served: 2,
      var: 2,
      lit: null,
      none: null,
    });
  });
});

describe("followCostCaps", () => {
  it("moves an untouched cap to the Run's new override and keeps a changed one", () => {
    const pipeline = { loops: [region("a", 8), region("b", 12), region("c", 3), region("v", "$budget")] };
    const baseline = costCapBaseline({ loops: [region("a", 8), region("b", 8), region("c", 3), region("v", "$budget")] }, {
      a: 8,
      b: 8,
      c: 3,
      v: 5,
    });
    const moved = followCostCaps(pipeline, baseline, { a: 4, b: 4, c: 3, v: 6 });
    expect(moved?.pipeline.loops.map((r) => r.max_cost_usd)).toEqual([4, 12, 3, 6]);
    expect(moved?.baseline).toEqual({
      a: { usd: 4, yaml: 4 },
      b: { usd: 4, yaml: 8 },
      c: { usd: 3, yaml: 3 },
      v: { usd: 6, yaml: 6 },
    });
  });

  it("returns null when no override moved", () => {
    const baseline = { a: { usd: 8, yaml: 8 } };
    expect(followCostCaps({ loops: [region("a", 8)] }, baseline, { a: 8 })).toBeNull();
    expect(followCostCaps({ loops: [region("a", 8)] }, baseline, undefined)).toBeNull();
  });

  it("tells a typed `$other` or `0` from the untouched value, and never moves a loop with no entry", () => {
    const baseline = { a: { yaml: "$nope" }, b: { usd: null, yaml: null } };
    const moved = followCostCaps({ loops: [region("a", "$other"), region("b", 0), region("x", 2)] }, baseline, {
      a: 4,
      b: 4,
      x: 4,
    });
    expect(moved?.pipeline.loops.map((r) => r.max_cost_usd)).toEqual(["$other", 0, 2]);
  });

  it("moves an unresolved $var the user left alone", () => {
    const moved = followCostCaps({ loops: [region("a", "$nope")] }, { a: { yaml: "$nope" } }, { a: 4 });
    expect(moved?.pipeline.loops[0].max_cost_usd).toBe(4);
  });
});
