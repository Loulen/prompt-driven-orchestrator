import { describe, expect, it } from "vitest";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { RunState } from "../../shared/generated/RunState";
import { regionCapUsd } from "./loopRegionCost";

const region: LoopRegion = { id: "r", kind: "bounded", members: ["a"], max_iter: 5, max_cost_usd: 5 };
const pipeline = { variables: {} };
const run = (over: Partial<RunState>) => ({ run_id: "x", ...over }) as unknown as RunState;
const served = (cap_usd: number | null) => ({ r: { cap_usd } }) as unknown as RunState["loop_costs"];

describe("regionCapUsd — the cap a region's header reads", () => {
  it("off a Run, the YAML literal", () => {
    expect(regionCapUsd(region, pipeline, null)).toBe(5);
  });

  it("on a followed Run, the live override first, its removal included, then the daemon's cap", () => {
    expect(regionCapUsd(region, pipeline, run({ loop_costs: served(5) }))).toBe(5);
    expect(regionCapUsd(region, pipeline, run({ region_max_cost_overrides: { r: 8 }, loop_costs: served(5) }))).toBe(8);
    expect(regionCapUsd(region, pipeline, run({ region_max_cost_overrides: { r: null }, loop_costs: served(5) }))).toBeNull();
    expect(regionCapUsd(region, pipeline, run({ loop_costs: served(4) }))).toBe(4);
  });

  it("while the Run is edited for itself, the tab's value, edits included", () => {
    const typed = { ...region, max_cost_usd: 8 };
    const followed = run({ region_max_cost_overrides: { r: 4 }, loop_costs: served(4) });
    expect(regionCapUsd(typed, pipeline, followed, true)).toBe(8);
    expect(regionCapUsd({ ...region, max_cost_usd: null }, pipeline, followed, true)).toBeNull();
  });

  it("never caps a collection", () => {
    const collection: LoopRegion = { id: "r", kind: "collection", members: ["a"], over: "items" };
    expect(regionCapUsd(collection, pipeline, run({ region_max_cost_overrides: { r: 3 } }))).toBeNull();
  });
});
