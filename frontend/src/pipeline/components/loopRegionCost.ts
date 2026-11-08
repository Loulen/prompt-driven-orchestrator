// What a loop region's corner says about money (ADR draft
// plafond-de-cout-de-boucle): its Coût de boucle on a Run, its Plafond de coût,
// the amber warning from 80 % of the cap, and the « budget reached » park.
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { RunState } from "../../shared/generated/RunState";
import type { EditorPipelineDef } from "../model/pipeline";
import {
  costNearCap,
  formatCostCap,
  loopCostLabel,
  loopCostWindow,
  type LoopCostLabel,
} from "../../run/model/loopCostLabel";

/** The money half of a region's layout. */
export interface RegionCost {
  /** The Coût de boucle on a Run, `null` off a Run and before a member ran. */
  cost: LoopCostLabel | null;
  /** The Plafond de coût (`$5`, or a `$var` verbatim off a Run), `null` uncapped. */
  cap: { text: string; usd: number | null } | null;
  /** The cost reached 80 % of the cap: the corner's cost turns amber. */
  costWarn: boolean;
  /** The Run is parked « budget reached » on THIS region. */
  budgetReached: boolean;
}

/**
 * The effective cost cap of a region, in dollars, as far as the client can
 * tell: the daemon's resolved `cap_usd` on a Run, else the YAML literal, else
 * the `$var`'s value — the one the Run was launched with, then the declared
 * default. `null` = uncapped, and always for a collection.
 */
export function regionCapUsd(
  region: LoopRegion,
  pipeline: Pick<EditorPipelineDef, "variables">,
  runState: RunState | null | undefined,
): number | null {
  if (region.kind !== "bounded") return null;
  const served = runState?.loop_costs?.[region.id]?.cap_usd;
  if (served != null) return served;
  const cap = region.max_cost_usd;
  if (cap == null || cap === "") return null;
  const name = typeof cap === "string" && cap.startsWith("$") ? cap.slice(1) : null;
  const raw =
    name == null
      ? cap
      : runState?.variables?.[name] !== undefined
        ? runState.variables[name]
        : pipeline.variables?.[name]?.default;
  const n = typeof raw === "number" ? raw : typeof raw === "string" ? Number(raw) : NaN;
  return Number.isFinite(n) && n > 0 ? n : null;
}

/** The corner's cost text: `~$1.20 / $5`, `~$1.20`, `≤ $5` off a Run, or none. */
export function regionCostText(region: Pick<RegionCost, "cost" | "cap">): string | null {
  if (region.cost && region.cap) return `${region.cost.text} / ${region.cap.text}`;
  if (region.cost) return region.cost.text;
  if (region.cap) return `≤ ${region.cap.text}`;
  return null;
}

/** Whether the Run is parked on `code` for `region` (historical logs carry no region id). */
export function parkedOn(
  runState: RunState | null | undefined,
  code: string,
  region: LoopRegion,
  fallback: boolean,
): boolean {
  if (runState?.status !== "awaiting_user" || runState.awaiting_reason_code !== code) return false;
  return runState.awaiting_region_id != null ? runState.awaiting_region_id === region.id : fallback;
}

/** The region a Run is parked « budget reached » on, or `null`. */
export function budgetReachedRegion(run: RunState | null | undefined): string | null {
  if (run?.status !== "awaiting_user" || run.awaiting_reason_code !== "region_budget_exhausted") return null;
  return run.awaiting_region_id ?? null;
}

export function deriveRegionCost(
  region: LoopRegion,
  pipeline: EditorPipelineDef,
  runState: RunState | null | undefined,
  currentIter: number,
): RegionCost {
  const collection = region.kind === "collection";
  const loopCost = runState != null ? runState.loop_costs?.[region.id] : undefined;
  const byId = new Map(pipeline.nodes.map((n) => [n.id, n]));
  const memberName = (id: string) =>
    byId.get(id)?.name ?? runState?.node_defs?.find((d) => d.id === id)?.name ?? id;
  const cost = loopCost ? loopCostLabel(loopCost, loopCostWindow(collection, currentIter), memberName) : null;
  const capUsd = regionCapUsd(region, pipeline, runState);
  const capRaw = region.max_cost_usd;
  const cap = collection
    ? null
    : capUsd != null
      ? { text: formatCostCap(capUsd), usd: capUsd }
      : capRaw != null && capRaw !== ""
        ? { text: String(capRaw), usd: null }
        : null;
  const spent = loopCost?.usd ?? null;
  const budgetReached =
    !collection &&
    parkedOn(runState, "region_budget_exhausted", region, capUsd != null && spent != null && spent >= capUsd);
  return { cost, cap, costWarn: budgetReached || costNearCap(spent, capUsd), budgetReached };
}
