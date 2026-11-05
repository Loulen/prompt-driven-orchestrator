import type { PipelineDef } from "../types";

/** One `set_region_max_cost` the « Save for this run » gesture must issue. */
export interface CostCapEdit {
  regionId: string;
  /** The new cap in dollars, or `null` when the cap was removed. */
  usd: number | null;
}

/** A YAML `max_cost_usd` read as a cap: a positive number, `null` = none, `undefined` = a `$var`. */
function capValue(raw: number | string | null | undefined): number | null | undefined {
  if (raw == null || raw === "") return null;
  if (typeof raw === "number") return Number.isFinite(raw) && raw > 0 ? raw : null;
  if (raw.startsWith("$")) return undefined;
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : null;
}

/**
 * #1017 — the cost-cap edits between the Run's snapshot `before` a save and the
 * pipeline being saved: one per bounded region whose `max_cost_usd` changed.
 * Hot editing a cap goes through the `set_region_max_cost` command (an override
 * in the event log, ADR-0081), so saving it also resumes a Run parked
 * « budget reached ». A `$var` cap is a pipeline expression only the daemon
 * resolves — it travels by the YAML alone. A `collection` is never capped (Q10).
 */
export function costCapEdits(before: PipelineDef, after: PipelineDef): CostCapEdit[] {
  const previous = new Map((before.loops ?? []).map((r) => [r.id, capValue(r.max_cost_usd)]));
  const edits: CostCapEdit[] = [];
  for (const region of after.loops ?? []) {
    if (region.kind !== "bounded") continue;
    const next = capValue(region.max_cost_usd);
    if (next === undefined) continue;
    const prev = previous.has(region.id) ? previous.get(region.id) : null;
    if (prev === next) continue;
    edits.push({ regionId: region.id, usd: next });
  }
  return edits;
}
