// The Coût de l'arbre (ADR-0101) as Run info and the
// Orchestration tab say it: the total, « Parent run » / « Child runs (N) » and,
// for a tree deeper than one level, each depth. Same markers as a Run's cost
// (`~` estimated, `†` lower bound, `—` unavailable), through `formatEstCost`.

import type { CostStat } from "../../shared/generated/CostStat";
import type { TreeCost } from "../../shared/generated/TreeCost";
import { formatEstCost } from "./costLabel";

/** One amount of a tree: its text (`~$0.80`, `—`) and whether it is a lower bound. */
export interface TreeAmount {
  text: string;
  dagger: boolean;
}

/** An amount read like a Run's cost: `—` when absent or not summable, never `$0`. */
export function treeAmount(cost: CostStat | null | undefined): TreeAmount {
  if (!cost) return { text: "—", dagger: false };
  const label = formatEstCost(
    cost.usd,
    cost.partial,
    cost.unpriced_models ?? [],
    cost.uncosted_harnesses ?? [],
    cost.by_harness ?? [],
  );
  return { text: label.text, dagger: label.dagger };
}

/** `~$0.80`, `~$0.80†`, `—`: the amount with its lower-bound marker, as plain text. */
export function amountText(amount: TreeAmount): string {
  return `${amount.text}${amount.dagger ? "†" : ""}`;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export interface TreeCostLabel {
  total: TreeAmount;
  parent: TreeAmount;
  children: TreeAmount;
  /** « Child runs (N) ». */
  childrenLabel: string;
  descendants: number;
  /** Each depth (« Depth 1 ~$2.10 »), only when the tree is deeper than one level. */
  depths: { label: string; amount: TreeAmount }[];
  /** Why the total reads `—`, `null` when it has one. */
  unavailable: string | null;
  /** Why the total is a lower bound, `null` when it is not one. */
  lowerBound: string | null;
}

/** Why a tree's total is withheld: a Run without a cost, or a harness without a cost source. */
function unavailableReason(tree: TreeCost): string | null {
  if (!tree.total) {
    const n = Math.max(tree.unavailable_runs, 1);
    return `The cost of ${plural(n, "run")} in this tree cannot be computed.`;
  }
  const uncosted = tree.total.uncosted_harnesses ?? [];
  if (uncosted.length > 0) {
    return `${uncosted.length === 1 ? "Harness" : "Harnesses"} ${uncosted.join(", ")} ${
      uncosted.length === 1 ? "has" : "have"
    } no cost source, so this tree's cost cannot be estimated.`;
  }
  return null;
}

export function treeCostLabel(tree: TreeCost): TreeCostLabel {
  const total = treeAmount(tree.total);
  const unpriced = tree.total?.unpriced_models ?? [];
  return {
    total,
    parent: treeAmount(tree.parent),
    children: treeAmount(tree.children),
    childrenLabel: `Child runs (${tree.descendants})`,
    descendants: tree.descendants,
    depths:
      tree.by_depth.length > 1
        ? tree.by_depth.map((d) => ({ label: `Depth ${d.depth}`, amount: treeAmount(d.cost) }))
        : [],
    unavailable: unavailableReason(tree),
    lowerBound:
      total.dagger && total.text !== "—"
        ? unpriced.length > 0
          ? `Lower bound: unpriced ${unpriced.length === 1 ? "model" : "models"} excluded: ${unpriced.join(", ")}.`
          : "Lower bound: an unpriced model was excluded."
        : null,
  };
}

/** The tooltip of a Run's Est. cost, line by line (the same lines its accessible name reads). */
export function runTreeCostLines(label: TreeCostLabel): string[] {
  const lines = [
    label.unavailable ? "Cost unavailable" : `Est. cost · tree ${amountText(label.total)}`,
    ...(label.unavailable ? [label.unavailable] : []),
    `Parent run ${amountText(label.parent)}`,
    `${label.childrenLabel} ${amountText(label.children)}`,
  ];
  if (label.depths.length > 0) {
    lines.push(label.depths.map((d) => `${d.label} ${amountText(d.amount)}`).join(" · "));
  }
  if (label.lowerBound) lines.push(label.lowerBound);
  if (!label.unavailable) {
    lines.push(`Includes this run and its ${plural(label.descendants, "child run")}.`);
  }
  return lines;
}

/** The tooltip of a child's `⑂ N` chip in the Orchestration tab. */
export function childTreeCostLines(label: TreeCostLabel): string[] {
  return [
    `Parent run ${amountText(label.parent)} · ${label.childrenLabel} ${amountText(label.children)}`,
    ...(label.unavailable ? [label.unavailable] : []),
    ...(label.lowerBound ? [label.lowerBound] : []),
    "Tree cost: this child run and its descendants.",
  ];
}
