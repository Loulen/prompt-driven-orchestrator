import type { RunListEntry } from "../types";

/* #336 — the client-side filter model for the Runs list. Option sets are
   derived from the runs themselves (never from the live library/trigger
   stores) so runs of renamed or deleted pipelines/triggers stay filterable.
   The daemon already ships every axis on `GET /runs` (`effective_repo`,
   `pipeline_name`, `triggered_by`); filtering is a pure view-layer computation.

   This lives in its own module (#345) so `RunFilters.tsx` only exports a
   component: the non-component exports below (an object constant and a
   function) would otherwise break React Fast Refresh
   (`react-refresh/only-export-components`), mirroring the `highlightYaml`
   → `yamlHighlight.tsx` split. */

/** Sentinel filter values for rows whose axis key is absent/empty. */
export const MANUAL_TRIGGER = "__manual__";
export const NONE = "__none__";

export interface RunFilterValue {
  repo: string | null;
  pipeline: string | null;
  /** A trigger id, or MANUAL_TRIGGER for manually launched runs. */
  trigger: string | null;
}

export const EMPTY_RUN_FILTER: RunFilterValue = {
  repo: null,
  pipeline: null,
  trigger: null,
};

/** The filter key for a run on each axis (empty/missing values bucket to a sentinel). */
export function repoKey(r: RunListEntry): string {
  return r.effective_repo && r.effective_repo.length > 0 ? r.effective_repo : NONE;
}
export function pipelineKey(r: RunListEntry): string {
  const name = r.pipeline_name ?? "";
  return name.trim().length > 0 ? name : NONE;
}
export function triggerKey(r: RunListEntry): string {
  return r.triggered_by ?? MANUAL_TRIGGER;
}

/** AND predicate over the three axes; a null axis means "All". */
export function runMatchesFilter(r: RunListEntry, f: RunFilterValue): boolean {
  if (f.repo !== null && repoKey(r) !== f.repo) return false;
  if (f.pipeline !== null && pipelineKey(r) !== f.pipeline) return false;
  if (f.trigger !== null && triggerKey(r) !== f.trigger) return false;
  return true;
}
