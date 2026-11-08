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
import { costCapEdits, foldCostCapOverrides } from "../model/runCostCaps";

/** What the Run applied before the save, or why it could not be read. */
export type AppliedCostCaps = { pipeline: EditorPipelineDef } | { error: string } | null;

/**
 * The Run's caps as it applies them, read before the save: its snapshot with
 * its live overrides folded in. `null` when the tab has no bounded loop (no
 * read at all).
 */
export async function appliedCostCaps(runId: string, tabPipeline: EditorPipelineDef): Promise<AppliedCostCaps> {
  if (!tabPipeline.loops?.some((r) => r.kind === "bounded")) return null;
  try {
    const [detail, run] = await Promise.all([fetchRunPipeline(runId), fetchRun(runId)]);
    return { pipeline: foldCostCapOverrides(detail.pipeline, run.region_max_cost_overrides) };
  } catch (err: unknown) {
    return { error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Issue one `set_region_max_cost` per cap the save changed. Resolves with what
 * the tab should say when a cap did not reach the Run, `null` when all did.
 */
export async function applyCostCaps(
  runId: string,
  before: AppliedCostCaps,
  saved: EditorPipelineDef,
): Promise<string | null> {
  if (before == null) return null;
  if ("error" in before) {
    return `Saved, but the run's cost caps could not be read, so a changed cap was not applied to it: ${before.error}`;
  }
  let message: string | null = null;
  for (const edit of costCapEdits(before.pipeline, saved)) {
    try {
      await setRegionMaxCost(runId, edit.regionId, edit.usd);
    } catch (err: unknown) {
      const why = err instanceof Error ? err.message : String(err);
      message = `Saved, but the cost cap of '${edit.regionId}' was not applied to the run: ${why}`;
    }
  }
  return message;
}
