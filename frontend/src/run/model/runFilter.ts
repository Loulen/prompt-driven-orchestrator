import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { ProjectRef } from "../../lib/groupByRepo";
import { ANY_TIME, runMatchesDate, type RunDateFilter } from "./runDateFilter";

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
  /** A {@link projectKey}: the Projet recorded on the Run, else its repo. */
  repo: string | null;
  pipeline: string | null;
  /** A trigger id, or MANUAL_TRIGGER for manually launched runs. */
  trigger: string | null;
  /** The Date axis (`runDateFilter.ts`): Active / Started over a range. */
  date: RunDateFilter;
}

/* #783 — the #725 `showOrchestrated` axis is gone: children are no longer a
   filterable population but rows nested under their parent (`lib/runTree.ts`),
   and the strip's fourth control is the expand/collapse-all button, which is
   NOT a filter (never accent, never part of `isFilterActive`). */
export const EMPTY_RUN_FILTER: RunFilterValue = {
  repo: null,
  pipeline: null,
  trigger: null,
  date: ANY_TIME,
};

/** The filter key for a run on each axis (empty/missing values bucket to a sentinel). */
export function repoKey(r: RunListEntry): string {
  return r.effective_repo && r.effective_repo.length > 0 ? r.effective_repo : NONE;
}
/** The Projet recorded on a Run (ADR-0087), as the list groups it. */
export type ProjectOfRun = (projectId: string | null | undefined) => ProjectRef | null;

const NO_PROJECT_KNOWN: ProjectOfRun = () => null;

/**
 * The filter key on the Project axis, the same buckets as the list's groups:
 * `project:<id>` for a Run launched in a Projet that still exists, else the
 * Run's repo ({@link repoKey}) — a repo in several Projets never mixes them.
 */
export function projectKey(r: RunListEntry, projectOf: ProjectOfRun = NO_PROJECT_KNOWN): string {
  const project = projectOf(r.project_id);
  return project ? `project:${project.id}` : repoKey(r);
}
export function pipelineKey(r: RunListEntry): string {
  const name = r.pipeline_name ?? "";
  return name.trim().length > 0 ? name : NONE;
}
export function triggerKey(r: RunListEntry): string {
  return r.triggered_by ?? MANUAL_TRIGGER;
}

/**
 * AND predicate over the four axes; a null axis means "All", a Date axis with
 * no range "any time" (`now` places its presets). On the tree
 * (#783) this is the per-run match: `filterRunTree` keeps a non-matching
 * parent as long as a descendant matches.
 */
export function runMatchesFilter(
  r: RunListEntry,
  f: RunFilterValue,
  projectOf: ProjectOfRun = NO_PROJECT_KNOWN,
  now: Date = new Date(),
): boolean {
  if (f.repo !== null && projectKey(r, projectOf) !== f.repo) return false;
  if (f.pipeline !== null && pipelineKey(r) !== f.pipeline) return false;
  if (f.trigger !== null && triggerKey(r) !== f.trigger) return false;
  return runMatchesDate(r, f.date, now);
}

/**
 * True when at least one axis narrows the list — the condition for the strip's
 * clear ✕ (and the empty state's).
 */
export function isFilterActive(f: RunFilterValue): boolean {
  return f.repo !== null || f.pipeline !== null || f.trigger !== null || f.date.range !== null;
}
