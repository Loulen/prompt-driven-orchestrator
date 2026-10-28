/**
 * Where a Run's worktree came from: its source branch and how far it has drifted.
 */

import { RefreshCw } from "lucide-react";
import { SourceDriftChip } from "./InspectorPrimitives";
import { useSourceDrift } from "../../run/store/useSourceDrift";
import type { RunState } from "../../shared/generated/RunState";
import { shortAge } from "../../workspace/model/branchSelect";

export function SourceBlock({ run }: { run: RunState }) {
  const { drift, fetching, fetchError, refresh } = useSourceDrift(run);
  if (!drift) return null;
  const available = drift.state === "available";

  return (
    <div
      className="mt-2 rounded border border-line-strong bg-surface-2 px-2 py-1.5"
      style={{ fontSize: "10.5px" }}
      data-testid="run-source-block"
    >
      <div className="flex items-center gap-2">
        <span className="shrink-0 text-text-3">Source</span>
        <span className="min-w-0 flex-1 truncate font-mono text-text-2">
          {available ? drift.source_branch : (run.source_branch ?? "—")}
        </span>
        <SourceDriftChip drift={drift} runId={run.run_id} fetchError={fetchError} />
      </div>
      <div className="mt-1 flex items-center gap-2 text-text-3" style={{ fontSize: "10px" }}>
        <span className="shrink-0">fork</span>
        <span className="min-w-0 flex-1 truncate font-mono">
          {available ? drift.fork : "—"}
        </span>
        {available ? (
          <>
            <span className="shrink-0">
              {shortAge(drift.last_fetch_at)
                ? `fetched ${shortAge(drift.last_fetch_at)} ago`
                : "never fetched"}
            </span>
            {run.target_repo && (
              <button
                type="button"
                onClick={() => void refresh()}
                disabled={fetching}
                className="shrink-0 rounded p-0.5 text-text-3 transition-colors hover:bg-hover hover:text-text-2 disabled:opacity-40"
                // Icon only: there is no decision to take here, only a refresh —
                // the popover of the launch form exists because THERE a choice
                // follows. Without the button, "fetched 2 h ago" is information
                // with no way out.
                title="Fetch all remotes of this repository, then recompute"
                aria-label="Fetch all remotes"
                data-testid="run-source-fetch"
              >
                <RefreshCw size={11} className={fetching ? "animate-spin" : undefined} />
              </button>
            )}
          </>
        ) : (
          <span className="shrink-0" data-testid="run-source-unavailable">
            {drift.reason}
          </span>
        )}
      </div>
    </div>
  );
}

