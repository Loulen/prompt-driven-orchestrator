// #1016 / ADR-0083 — the Run panel's wording of a `region_budget_exhausted`
// park: « Loop <id> reached its cost cap: ~$5.12 spent of $5 after lap 4/5. »
// Built from the structured Run state (the region the park names, its loop cost
// and resolved cap, its lap) so it reads in the canvas' own cost vocabulary
// (`lib/costLabel.ts`); the daemon's prose stays the fallback.

import type { PipelineDef, RunState } from "../types";
import { formatCostAmount, formatCostCap } from "./costLabel";

export const BUDGET_REASON_CODE = "region_budget_exhausted";

export interface BudgetReason {
  regionId: string;
  /** `~$5.12` (`†` when partial, `—` unknown) — {@link formatCostAmount}. */
  spent: string;
  /** `$5` — {@link formatCostCap}; `null` when the daemon served no cap. */
  cap: string | null;
  /** `4/5`, or `4` when the bound is unknown; `null` when no lap is known. */
  lap: string | null;
  partial: boolean;
}

/** The structured budget park of `run`, or `null` when it is not parked on one. */
export function budgetReason(
  run: RunState | null | undefined,
  pipeline?: PipelineDef | null,
): BudgetReason | null {
  if (!run || run.awaiting_reason_code !== BUDGET_REASON_CODE) return null;
  const regionId = run.awaiting_region_id;
  if (!regionId) return null;
  const cost = run.loop_costs?.[regionId];
  const state = run.loop_states?.[regionId];
  // The region counter when projected; else the max iter across its members
  // (the daemon stamps every member with the region's lap), over the YAML bound.
  const region = pipeline?.loops?.find((r) => r.id === regionId);
  const memberLap = region
    ? region.members.reduce((max, id) => Math.max(max, run.nodes[id]?.iter ?? 0), 0)
    : 0;
  const lap = state
    ? `${state.current_iter}/${state.max_iter}`
    : memberLap > 0
      ? region?.max_iter != null
        ? `${memberLap}/${region.max_iter}`
        : `${memberLap}`
      : null;
  return {
    regionId,
    spent: cost ? formatCostAmount(cost.usd, cost.partial && cost.usd !== null, cost.estimated) : "—",
    cap: cost?.cap_usd != null ? formatCostCap(cost.cap_usd) : null,
    lap,
    partial: cost?.partial ?? false,
  };
}
