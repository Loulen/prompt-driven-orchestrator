// The Coût de l'arbre as Run info and the Orchestration tab word it: same
// markers as a Run's cost, the breakdown by depth only past one level.
import { describe, expect, it } from "vitest";
import type { CostStat } from "../../shared/generated/CostStat";
import type { TreeCost } from "../../shared/generated/TreeCost";
import { childTreeCostLines, runTreeCostLines, treeAmount, treeCostLabel } from "./treeCost";

const usd = (amount: number, over: Partial<CostStat> = {}): CostStat => ({
  usd: amount,
  partial: false,
  unpriced_models: [],
  uncosted_harnesses: [],
  by_harness: [],
  ...over,
});

const tree = (over: Partial<TreeCost> = {}): TreeCost => ({
  total: usd(4.2),
  parent: usd(1.8),
  children: usd(2.4),
  descendants: 4,
  by_depth: [{ depth: 1, runs: 4, cost: usd(2.4) }],
  unavailable_runs: 0,
  ...over,
});

describe("treeAmount", () => {
  it("reads an amount like a Run's cost, « — » when absent or not summable", () => {
    expect(treeAmount(usd(4.2))).toEqual({ text: "~$4.20", dagger: false });
    expect(treeAmount(usd(4.2, { partial: true, unpriced_models: ["mystery"] }))).toEqual({ text: "~$4.20", dagger: true });
    expect(treeAmount(null)).toEqual({ text: "—", dagger: false });
    expect(treeAmount(usd(0, { uncosted_harnesses: ["opencode"] }))).toEqual({ text: "—", dagger: false });
  });
});

describe("runTreeCostLines", () => {
  it("says the total, Parent run, Child runs (N) and what it includes", () => {
    expect(runTreeCostLines(treeCostLabel(tree()))).toEqual([
      "Est. cost · tree ~$4.20",
      "Parent run ~$1.80",
      "Child runs (4) ~$2.40",
      "Includes this run and its 4 child runs.",
    ]);
  });

  it("adds each depth only when the tree is deeper than one level", () => {
    const deep = tree({
      by_depth: [
        { depth: 1, runs: 3, cost: usd(1.1) },
        { depth: 2, runs: 1, cost: usd(1.3) },
      ],
    });
    expect(runTreeCostLines(treeCostLabel(deep))).toContain("Depth 1 ~$1.10 · Depth 2 ~$1.30");
  });

  it("names how many Runs make the total unavailable, and keeps the known slices", () => {
    const lines = runTreeCostLines(
      treeCostLabel(tree({ total: null, children: null, unavailable_runs: 1, descendants: 1 })),
    );
    expect(lines).toEqual([
      "Cost unavailable",
      "The cost of 1 run in this tree cannot be computed.",
      "Parent run ~$1.80",
      "Child runs (1) —",
    ]);
  });

  it("names the harness without a cost source that withholds the total", () => {
    const label = treeCostLabel(tree({ total: usd(0, { uncosted_harnesses: ["opencode"] }) }));
    expect(label.total.text).toBe("—");
    expect(label.unavailable).toContain("opencode");
  });

  it("marks a lower bound with its dagger and names the unpriced model", () => {
    const label = treeCostLabel(tree({ total: usd(4.2, { partial: true, unpriced_models: ["mystery"] }) }));
    expect(label.total).toEqual({ text: "~$4.20", dagger: true });
    expect(runTreeCostLines(label)).toContain("Lower bound: unpriced model excluded: mystery.");
    expect(runTreeCostLines(label)[0]).toBe("Est. cost · tree ~$4.20†");
  });
});

describe("childTreeCostLines", () => {
  it("splits a child's tree cost and says what it covers", () => {
    expect(childTreeCostLines(treeCostLabel(tree({ descendants: 1 })))).toEqual([
      "Parent run ~$1.80 · Child runs (1) ~$2.40",
      "Tree cost: this child run and its descendants.",
    ]);
  });
});
