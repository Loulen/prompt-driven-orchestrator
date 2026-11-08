import { describe, expect, it } from "vitest";
import type { LoopCost } from "../../shared/generated/LoopCost";
import { costCapError, costNearCap, formatCostCap, loopCostLabel, loopCostWindow } from "./loopCostLabel";

function cost(over: Partial<LoopCost> = {}): LoopCost {
  return {
    usd: 1.2,
    estimated: true,
    partial: false,
    executions: 3,
    readable_executions: 3,
    members: [
      { node_id: "impl", usd: 0.7, estimated: true, partial: false, executions: 2 },
      { node_id: "rev", usd: 0.5, estimated: true, partial: false, executions: 1 },
    ],
    ...over,
  };
}

describe("loopCostLabel", () => {
  it("reads like the Run cost: ~ for an estimate, † for a lower bound, — for unknown", () => {
    expect(loopCostLabel(cost(), "laps 1–3").text).toBe("~$1.20");
    expect(loopCostLabel(cost({ partial: true }), "laps 1–3").text).toBe("~$1.20†");
    expect(loopCostLabel(cost({ usd: null, partial: false }), "laps 1–3").text).toBe("—");
    expect(loopCostLabel(cost({ estimated: false }), "laps 1–3").text).toBe("$1.20");
    expect(loopCostLabel(cost({ usd: 0.84 }), "2 items").text).toBe("~$0.8400");
  });

  it("details each member's share under what the cost covers, by name", () => {
    const { title } = loopCostLabel(cost(), "laps 1–3", (id) => (id === "impl" ? "implement" : "review"));
    expect(title).toMatch(/^Loop cost, laps 1–3: Estimate/);
    expect(title).toContain("\nimplement ~$0.7000 · review ~$0.5000");
  });

  it("says why a lower bound is one", () => {
    const { title } = loopCostLabel(
      cost({
        partial: true,
        readable_executions: 2,
        unavailable_reasons: ["harness has no cost source"],
        unpriced_models: ["gpt-x"],
      }),
      "lap 1",
    );
    expect(title).toContain("unpriced model excluded: gpt-x");
    expect(title).toContain("1 member execution has no readable cost (harness has no cost source)");
  });

  it("names the window of a bounded loop and of a collection", () => {
    expect(loopCostWindow(false, 1)).toBe("lap 1");
    expect(loopCostWindow(false, 3)).toBe("laps 1–3");
    expect(loopCostWindow(true, 1)).toBe("1 item");
    expect(loopCostWindow(true, 2)).toBe("2 items");
  });
});

describe("the cost cap", () => {
  it("writes a cap like the design: $5, $2.50, $0.0050", () => {
    expect(formatCostCap(5)).toBe("$5");
    expect(formatCostCap(2.5)).toBe("$2.50");
    expect(formatCostCap(0.005)).toBe("$0.0050");
  });

  it("turns amber from 80 % of the cap", () => {
    expect(costNearCap(3.99, 5)).toBe(false);
    expect(costNearCap(4, 5)).toBe(true);
    expect(costNearCap(null, 5)).toBe(false);
    expect(costNearCap(4, null)).toBe(false);
  });

  it("refuses a cap at or below 0 and a value that is neither a number nor a $var", () => {
    expect(costCapError("")).toBeNull();
    expect(costCapError("5")).toBeNull();
    expect(costCapError("0.5")).toBeNull();
    expect(costCapError("$budget")).toBeNull();
    expect(costCapError("0")).toBe("Must be above 0");
    expect(costCapError("-2")).toBe("Must be above 0");
    expect(costCapError("five")).toBe("A number or a $var");
    expect(costCapError("$")).toBe("A number or a $var");
  });

  it("refuses a $var the pipeline does not declare, when it knows its variables", () => {
    expect(costCapError("$budget", ["budget"])).toBeNull();
    expect(costCapError("$nope", ["budget"])).toBe("No such variable");
  });
});
