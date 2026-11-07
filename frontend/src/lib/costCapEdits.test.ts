import { describe, expect, it } from "vitest";
import type { LoopRegion, PipelineDef } from "../types";
import { costCapEdits } from "./costCapEdits";

function pipe(...loops: LoopRegion[]): PipelineDef {
  return { name: "p", variables: {}, nodes: [], edges: [], loops };
}
const bounded = (id: string, max_cost_usd?: number | string | null): LoopRegion => ({
  id,
  kind: "bounded",
  members: ["a"],
  max_iter: 5,
  ...(max_cost_usd !== undefined ? { max_cost_usd } : {}),
});

describe("costCapEdits (#1017)", () => {
  it("emits nothing when no cap changed", () => {
    expect(costCapEdits(pipe(bounded("L", 5)), pipe(bounded("L", 5)))).toEqual([]);
    expect(costCapEdits(pipe(bounded("L")), pipe(bounded("L", null)))).toEqual([]);
  });

  it("emits a raise, a lowering and a new cap", () => {
    expect(costCapEdits(pipe(bounded("L", 0.01)), pipe(bounded("L", 5)))).toEqual([
      { regionId: "L", usd: 5 },
    ]);
    expect(costCapEdits(pipe(bounded("L", 5)), pipe(bounded("L", 1)))).toEqual([
      { regionId: "L", usd: 1 },
    ]);
    expect(costCapEdits(pipe(bounded("L")), pipe(bounded("L", 2)))).toEqual([
      { regionId: "L", usd: 2 },
    ]);
  });

  it("emits a removal as null", () => {
    expect(costCapEdits(pipe(bounded("L", 5)), pipe(bounded("L", null)))).toEqual([
      { regionId: "L", usd: null },
    ]);
    expect(costCapEdits(pipe(bounded("L", 5)), pipe(bounded("L")))).toEqual([
      { regionId: "L", usd: null },
    ]);
  });

  it("leaves a `$var` cap to the YAML, and never caps a collection", () => {
    expect(costCapEdits(pipe(bounded("L", 5)), pipe(bounded("L", "$budget")))).toEqual([]);
    const coll: LoopRegion = { id: "C", kind: "collection", members: ["a"], over: "items" };
    expect(costCapEdits(pipe(coll), pipe({ ...coll, max_cost_usd: 5 }))).toEqual([]);
  });
});
