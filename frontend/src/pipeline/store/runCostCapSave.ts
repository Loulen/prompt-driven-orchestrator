/**
 * « Save for this run » and the Plafond de coût (ADR draft
 * plafond-de-cout-de-boucle): a cap changed for this run goes through
 * `set_region_max_cost` (an override the Run keeps), which is also what
 * resumes a Run parked « budget reached ». The YAML is saved either way.
 */

import { fetchRun } from "../../run/api/runs";
import { setRegionMaxCost } from "../../run/api/commands";
import { fetchRunPipeline } from "../../run/api/runPipeline";
import type { EditorPipelineDef } from "../model/pipeline";
import {
  costCapEdits,
  runAppliedCaps,
  savedCostCapBaseline,
  type CostCapEdit,
  type RunCostCapBaseline,
} from "../model/runCostCaps";

/** The caps the Run applied before the save, or why they could not be read. */
export type AppliedCostCaps = { applied: Record<string, number | null> } | { error: string } | null;

/**
 * The Run's caps as it applies them, read before the save: its overrides,
 * then the caps of its snapshot (a `$var` resolved). `null` when the tab has
 * no bounded loop (no read at all).
 */
export async function appliedCostCaps(runId: string, tabPipeline: EditorPipelineDef): Promise<AppliedCostCaps> {
  if (!tabPipeline.loops?.some((r) => r.kind === "bounded")) return null;
  try {
    const [detail, run] = await Promise.all([fetchRunPipeline(runId), fetchRun(runId)]);
    return { applied: runAppliedCaps(detail.pipeline, run) };
  } catch (err: unknown) {
    return { error: err instanceof Error ? err.message : String(err) };
  }
}

/** What applying the caps did: what the tab should say, and what it now took from the Run. */
export interface CostCapsApplied {
  /** Why a cap did not reach the Run, `null` when all did. */
  message: string | null;
  /** The tab's new baseline, `undefined` when the Run's caps are unknown. */
  baseline?: RunCostCapBaseline;
}

/** Issue one `set_region_max_cost` per cap the user changed in the saved pipeline. */
export async function applyCostCaps(
  runId: string,
  before: AppliedCostCaps,
  saved: EditorPipelineDef,
  baseline: RunCostCapBaseline | undefined,
): Promise<CostCapsApplied> {
  if (before == null) return { message: null };
  if ("error" in before) {
    return {
      message: `Saved, but the run's cost caps could not be read, so a changed cap was not applied to it: ${before.error}`,
    };
  }
  let message: string | null = null;
  const taken: CostCapEdit[] = [];
  const refused: string[] = [];
  for (const edit of costCapEdits(saved, baseline, before.applied)) {
    try {
      await setRegionMaxCost(runId, edit.regionId, edit.usd);
      taken.push(edit);
    } catch (err: unknown) {
      const why = err instanceof Error ? err.message : String(err);
      message = `Saved, but the cost cap of '${edit.regionId}' was not applied to the run: ${why}`;
      refused.push(edit.regionId);
    }
  }
  return { message, baseline: savedCostCapBaseline(saved, before.applied, taken, refused, baseline) };
}

/**
 * The baseline a Save leaves: `saved`, except for a loop the tab followed
 * while the save ran (its entry is no longer the one the save started from,
 * `took`), which keeps what it followed (`now`).
 */
export function keepFollowed(
  saved: RunCostCapBaseline,
  took: RunCostCapBaseline | undefined,
  now: RunCostCapBaseline | undefined,
): RunCostCapBaseline {
  if (!now || now === took) return saved;
  const out = { ...saved };
  for (const [id, entry] of Object.entries(now)) {
    if (entry !== took?.[id]) out[id] = entry;
  }
  return out;
}
