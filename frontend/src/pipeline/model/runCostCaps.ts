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

/** What a Run says about its caps: its overrides, its served caps, its variables. */
export interface RunCapSource {
  region_max_cost_overrides?: CostCapOverrides | null;
  loop_costs?: Readonly<Record<string, { cap_usd?: number | null }>> | null;
  variables?: Readonly<Record<string, unknown>> | null;
}

interface WithVariables {
  variables?: Readonly<Record<string, { default?: unknown } | undefined>> | null;
}

/**
 * A `$var` cap's value in dollars: the one the Run was launched with, then the
 * declared default. `null` when it does not resolve to a positive amount.
 */
export function resolveCapVariable(
  name: string,
  pipeline: WithVariables,
  run: Pick<RunCapSource, "variables"> | null | undefined,
): number | null {
  const raw = run?.variables?.[name] !== undefined ? run.variables[name] : pipeline.variables?.[name]?.default;
  const n = typeof raw === "number" ? raw : typeof raw === "string" ? Number(raw) : NaN;
  return Number.isFinite(n) && n > 0 ? n : null;
}

/**
 * The cap each bounded loop of a Run applies, in dollars (`null`: uncapped):
 * its live override, else the daemon's resolved cap, else the YAML literal or
 * the `$var`'s value: what tells, at Save and when the Run moves, which caps
 * the user changed (see {@link costCapEdits}). A `$var` nothing resolves stays
 * out.
 */
export function runAppliedCaps(
  pipeline: WithLoops & WithVariables,
  run: RunCapSource | null | undefined,
): Record<string, number | null> {
  const applied: Record<string, number | null> = {};
  for (const region of pipeline.loops ?? []) {
    if (region.kind !== "bounded") continue;
    const overrides = run?.region_max_cost_overrides;
    if (overrides && region.id in overrides) {
      applied[region.id] = overrides[region.id] ?? null;
      continue;
    }
    const served = run?.loop_costs?.[region.id]?.cap_usd;
    if (served != null) {
      applied[region.id] = served;
      continue;
    }
    const raw = region.max_cost_usd;
    if (typeof raw === "string" && raw.startsWith("$")) {
      const resolved = resolveCapVariable(raw.slice(1), pipeline, run);
      if (resolved != null) applied[region.id] = resolved;
      continue;
    }
    applied[region.id] = capValue(raw) ?? null;
  }
  return applied;
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

/**
 * What a tab edited « for this run » took from its Run for one loop: the cap
 * the Run applied (`usd`; absent for a `$var` nothing resolves) and the
 * `max_cost_usd` the tab held then (`yaml`). A loop whose `max_cost_usd` is
 * still `yaml` is untouched: it follows the Run, and a Save leaves the Run's
 * cap alone.
 */
export interface RunCostCap {
  usd?: number | null;
  yaml: LoopRegion["max_cost_usd"];
}

export type RunCostCapBaseline = Record<string, RunCostCap>;

function sameYaml(a: LoopRegion["max_cost_usd"], b: LoopRegion["max_cost_usd"]): boolean {
  return (a ?? null) === (b ?? null);
}

/** The baseline of a tab that holds `pipeline` while its Run applies `applied`. */
export function costCapBaseline(pipeline: WithLoops, applied: Readonly<Record<string, number | null>>): RunCostCapBaseline {
  const baseline: RunCostCapBaseline = {};
  for (const region of pipeline.loops ?? []) {
    if (region.kind !== "bounded") continue;
    baseline[region.id] = region.id in applied
      ? { usd: applied[region.id], yaml: region.max_cost_usd }
      : { yaml: region.max_cost_usd };
  }
  return baseline;
}

/**
 * A tab edited « for this run » follows a cap changed outside it (the CLI, an
 * extension, another browser): every loop whose override moved off what the
 * tab took takes the new cap, unless the user already changed it (their draft
 * wins, and their Save applies it). Returns the pipeline (itself when nothing
 * followed) and the new baseline, or `null` when no override moved.
 */
export function followCostCaps<P extends WithLoops>(
  pipeline: P,
  baseline: Readonly<RunCostCapBaseline>,
  overrides: CostCapOverrides | null | undefined,
): { pipeline: P; baseline: RunCostCapBaseline } | null {
  // A loop with no entry is the user's for good (a cap the Run refused).
  const moved = Object.entries(overrides ?? {}).filter(([id, usd]) => id in baseline && baseline[id].usd !== (usd ?? null));
  if (moved.length === 0) return null;
  const next = { ...baseline };
  const followed: Record<string, number | null> = {};
  for (const [id, raw] of moved) {
    const usd = raw ?? null;
    const region = pipeline.loops?.find((r) => r.id === id);
    if (region?.kind === "bounded" && sameYaml(region.max_cost_usd, baseline[id].yaml)) {
      followed[id] = usd;
      next[id] = { usd, yaml: usd };
    } else {
      next[id] = { usd, yaml: baseline[id].yaml };
    }
  }
  return { pipeline: foldCostCapOverrides(pipeline, followed), baseline: next };
}

/** A history entry follows the same way, or an undo would bring a stale cap back. */
export function followCostCapsIn<P extends WithLoops>(
  pipeline: P,
  baseline: Readonly<RunCostCapBaseline>,
  overrides: CostCapOverrides | null | undefined,
): P {
  return followCostCaps(pipeline, baseline, overrides)?.pipeline ?? pipeline;
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
 * The `set_region_max_cost` a « Save for this run » issues: one per bounded
 * loop the user changed (its `max_cost_usd` is no longer what the tab took,
 * `baseline`) to a cap other than the one the Run applies now (`applied`, read
 * at Save). An untouched loop issues nothing, so a cap changed outside the tab
 * and not followed yet is never put back. Editing a cap goes through this
 * override, which is also what resumes a Run parked « budget reached ». A
 * `$var` cap is an expression only the daemon resolves: it travels by the
 * YAML alone.
 */
export function costCapEdits(
  saved: WithLoops,
  baseline: Readonly<RunCostCapBaseline> | null | undefined,
  applied: Readonly<Record<string, number | null>>,
): CostCapEdit[] {
  const edits: CostCapEdit[] = [];
  for (const region of saved.loops ?? []) {
    if (region.kind !== "bounded") continue;
    const took = baseline?.[region.id];
    if (took && sameYaml(region.max_cost_usd, took.yaml)) continue; // untouched
    const next = capValue(region.max_cost_usd);
    if (next === undefined) continue;
    const now = region.id in applied ? applied[region.id] : null;
    if (now === next) continue;
    edits.push({ regionId: region.id, usd: next });
  }
  return edits;
}

/**
 * The baseline after a Save: the tab holds `saved`, and the Run applies
 * `applied` with the `edits` it took on top. A loop whose edit the Run
 * `refused` keeps what the tab took before (`previous`): still the user's
 * change, so the next Save tries it again, and untouched again once undone.
 */
export function savedCostCapBaseline(
  saved: WithLoops,
  applied: Readonly<Record<string, number | null>>,
  edits: readonly CostCapEdit[],
  refused: readonly string[] = [],
  previous?: Readonly<RunCostCapBaseline>,
): RunCostCapBaseline {
  const after: Record<string, number | null> = { ...applied };
  for (const e of edits) after[e.regionId] = e.usd;
  const baseline = costCapBaseline(saved, after);
  for (const id of refused) {
    if (previous?.[id]) baseline[id] = previous[id];
    else delete baseline[id];
  }
  return baseline;
}
