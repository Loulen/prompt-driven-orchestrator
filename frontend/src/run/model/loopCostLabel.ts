// The **Coût de boucle** and the **Plafond de coût** as the canvas and the
// region inspector say them (ADR draft plafond-de-cout-de-boucle): the Run
// cost's vocabulary (`~` estimate, `†` lower bound, `—` unknown, never `$0`),
// a cap written `$5`, and the 80 % threshold where the cost turns amber.

import type { LoopCost } from "../../shared/generated/LoopCost";
import {
  COST_ESTIMATE_NOTE,
  COST_LOWER_BOUND_NOTE,
  COST_REPORTED_IN_USD_NOTE,
  formatCostAmount,
} from "./costLabel";

/** A loop cost ready to render in a region corner or a one-member badge. */
export interface LoopCostLabel {
  /** `~$1.20`, `~$1.20†` (lower bound), `$1.20` (exact reported dollars), `—`. */
  text: string;
  /** Tooltip: what the figure is, then each member's share. */
  title: string;
}

/** The share of the cap at which the loop cost turns amber. */
export const COST_CAP_WARN_RATIO = 0.8;

/** What the cost covers: `lap 1`, `laps 1–3`, `2 items`. */
export function loopCostWindow(collection: boolean, currentIter: number): string {
  if (collection) return `${currentIter} ${currentIter === 1 ? "item" : "items"}`;
  return currentIter > 1 ? `laps 1–${currentIter}` : "lap 1";
}

function unpricedClause(unpriced: string[]): string {
  return ` Lower bound: unpriced ${unpriced.length === 1 ? "model" : "models"} excluded: ${unpriced.join(", ")}.`;
}

/**
 * The Coût de boucle label. `window` names what the cost covers
 * ({@link loopCostWindow}); `memberName` turns a member id into the name the
 * canvas shows.
 */
export function loopCostLabel(
  cost: LoopCost,
  window: string,
  memberName: (nodeId: string) => string = (id) => id,
): LoopCostLabel {
  const text = formatCostAmount(cost.usd, cost.partial, cost.estimated);
  const base =
    cost.usd === null
      ? "Cost projected from attributed harness contributions."
      : cost.estimated
        ? COST_ESTIMATE_NOTE
        : COST_REPORTED_IN_USD_NOTE;
  const unreadable = cost.executions - cost.readable_executions;
  const reasons = cost.unavailable_reasons ?? [];
  const unpriced = cost.unpriced_models ?? [];
  // A lower bound says every cause it has: unpriced models AND unreadable runs.
  const unreadableClause =
    unreadable > 0
      ? ` Lower bound: ${unreadable} member ${
          unreadable === 1 ? "execution has" : "executions have"
        } no readable cost${reasons.length > 0 ? ` (${reasons.join("; ")})` : ""}.`
      : "";
  const lowerBound = !cost.partial
    ? ""
    : unpriced.length > 0 || unreadableClause
      ? `${unpriced.length > 0 ? unpricedClause(unpriced) : ""}${unreadableClause}`
      : COST_LOWER_BOUND_NOTE;
  const unavailable =
    cost.usd === null && reasons.length > 0 ? ` Cost unavailable: ${reasons.join("; ")}.` : "";
  const members = cost.members
    .map(
      (m) =>
        `${memberName(m.node_id)} ${formatCostAmount(m.usd, m.partial && m.usd !== null, m.estimated)}`,
    )
    .join(" · ");
  return {
    text,
    title: `Loop cost, ${window}: ${base}${lowerBound}${unavailable}${members ? `\n${members}` : ""}`,
  };
}

/**
 * A Plafond de coût as the header and the inspector say it: `$5` for a
 * whole-dollar cap, `$2.50` / `$0.01` otherwise (4 decimals only below a cent,
 * so a sub-cent cap never reads `$0.00`).
 */
export function formatCostCap(usd: number): string {
  if (Number.isInteger(usd)) return `$${usd}`;
  return `$${usd.toFixed(usd < 0.01 ? 4 : 2)}`;
}

/** Whether `spent` is at or past the amber threshold of `cap`. */
export function costNearCap(spent: number | null, cap: number | null): boolean {
  return spent != null && cap != null && spent >= cap * COST_CAP_WARN_RATIO;
}

/**
 * Why a `max_cost_usd` the author typed is refused, or `null` when it is
 * valid (empty = no cap). The daemon refuses the same values at save.
 * `variables`: the pipeline's declared variables, when known.
 */
export function costCapError(raw: string, variables?: readonly string[]): string | null {
  const value = raw.trim();
  if (value === "") return null;
  if (value.startsWith("$")) {
    if (!/^\$[A-Za-z_][A-Za-z0-9_]*$/.test(value)) return "A number or a $var";
    return variables && !variables.includes(value.slice(1)) ? "No such variable" : null;
  }
  const n = Number(value);
  if (!Number.isFinite(n)) return "A number or a $var";
  return n > 0 ? null : "Must be above 0";
}
