// The Plafond de coût of a Run's loops edited « for this run » (ADR draft
// plafond-de-cout-de-boucle): the live `set_region_max_cost` overrides the Run
// carries, folded into what the editor shows, and the commands a Save issues.
import type { LoopRegion } from "../../shared/generated/LoopRegion";

/** A Run's live cost-cap overrides: a dollar cap, or `null` when it was removed. */
export type CostCapOverrides = Readonly<Record<string, number | null>>;

/** One `set_region_max_cost` a « Save for this run » issues. */
export interface CostCapEdit {
  regionId: string;
  /** The new cap in dollars, or `null` when the cap was removed. */
  usd: number | null;
}

interface WithLoops {
  loops?: LoopRegion[] | null;
}

/**
 * The pipeline with the Run's live overrides in place of the declared
 * `max_cost_usd`: what the Run actually applies, so « Edit for this run »
 * starts from it. A collection is never capped, so it is left alone.
 */
export function foldCostCapOverrides<P extends WithLoops>(
  pipeline: P,
  overrides: CostCapOverrides | null | undefined,
): P {
  const loops = pipeline.loops;
  if (!overrides || !loops?.some((r) => r.kind === "bounded" && r.id in overrides)) {
    return pipeline;
  }
  return {
    ...pipeline,
    loops: loops.map((region) =>
      region.kind === "bounded" && region.id in overrides
        ? { ...region, max_cost_usd: overrides[region.id] ?? null }
        : region,
    ),
  } as P;
}

/** A YAML `max_cost_usd` read as a cap: a positive number, `null` = none, `undefined` = a `$var`. */
function capValue(raw: LoopRegion["max_cost_usd"]): number | null | undefined {
  if (raw == null || raw === "") return null;
  if (typeof raw === "number") return Number.isFinite(raw) && raw > 0 ? raw : null;
  if (typeof raw !== "string") return null;
  if (raw.startsWith("$")) return undefined;
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : null;
}

/**
 * The cost-cap edits between what the Run applied `before` a save (its
 * snapshot with its overrides folded in) and the pipeline being saved: one per
 * bounded region whose cap changed. Editing a cap goes through
 * `set_region_max_cost` (an override in the event log), which is also what
 * resumes a Run parked « budget reached ». A `$var` cap is an expression only
 * the daemon resolves: it travels by the YAML alone.
 */
export function costCapEdits(before: WithLoops, after: WithLoops): CostCapEdit[] {
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
