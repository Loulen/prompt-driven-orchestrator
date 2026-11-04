import { useEffect, useState } from "react";
import { fetchRun } from "../../run/api/runs";
import { fetchRunPipeline } from "../../run/api/runPipeline";
import type { EditorPipelineDef } from "../../pipeline/model/pipeline";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { RunState } from "../../shared/generated/RunState";
import { mostRecentRun } from "../model/runsListPreview";

/** The most recent Run the user sees, read in full for the previews of Settings › Appearance. */
export interface RecentRun {
  run: RunState;
  /** Its own pipeline; `null` when it could not be read. */
  pipeline: EditorPipelineDef | null;
}

/**
 * The most recent Run of `runs`, its state and its pipeline, read once per Run:
 * `null` while there is none, while it loads, or when its state cannot be read
 * (the previews then show the sample).
 */
export function useRecentRun(runs: RunListEntry[]): { recent: RecentRun | null; none: boolean } {
  const runId = mostRecentRun(runs)?.run_id ?? null;
  const [recent, setRecent] = useState<RecentRun | null>(null);
  useEffect(() => {
    if (!runId) return;
    let cancelled = false;
    void Promise.all([fetchRun(runId), fetchRunPipeline(runId).catch(() => null)])
      .then(([run, detail]) => {
        if (!cancelled) setRecent({ run, pipeline: detail?.pipeline ?? null });
      })
      .catch(() => {
        if (!cancelled) setRecent(null);
      });
    return () => {
      cancelled = true;
    };
  }, [runId]);
  const shown = recent && recent.run.run_id === runId ? recent : null;
  return { recent: shown, none: runId === null };
}
