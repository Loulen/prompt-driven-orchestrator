/**
 * Where a Run's worktree came from: its source branch and how far it has drifted.
 */

import { RefreshCw } from "lucide-react";
import { IconButton, KeyValue } from "../../shared/ui/kit";
import { SourceDriftChip } from "./InspectorPrimitives";
import type { useSourceDrift } from "../../run/store/useSourceDrift";
import type { RunState } from "../../shared/generated/RunState";
import type { ElementMarks } from "../../hooks/useElementMarks";
import { shortAge } from "../../workspace/model/branchSelect";

/** The refresh icon, turning while the fetch runs. */
function Fetching(props: { size?: number | string; strokeWidth?: number | string }) {
  return <RefreshCw {...props} className="d-spin" />;
}

/**
 * `source` is the Run's drift as {@link useSourceDrift} reads it, read by the
 * caller so it knows whether anything shows.
 */
export function SourceBlock({
  run,
  source,
  marks,
}: {
  run: RunState;
  source: ReturnType<typeof useSourceDrift>;
  marks?: ElementMarks;
}) {
  const { drift, fetching, fetchError, refresh } = source;
  if (!drift) return null;
  const available = drift.state === "available";
  const branch = available ? drift.source_branch : (run.source_branch ?? "—");
  const age = available ? shortAge(drift.last_fetch_at) : null;

  return (
    <div data-testid="run-source-block" {...marks}>
      <KeyValue
        rows={[
          {
            k: "Source",
            tip: branch,
            v: (
              <>
                <span className="d-mono ei-trunc">{branch}</span>
                <SourceDriftChip drift={drift} runId={run.run_id} fetchError={fetchError} />
              </>
            ),
          },
          {
            k: "Fork",
            tip: available ? drift.fork : drift.reason,
            v: available ? (
              <>
                <span className="d-mono ei-trunc">{drift.fork}</span>
                <span className="ei-meta">{age ? `fetched ${age} ago` : "never fetched"}</span>
                {run.target_repo && (
                  // Icon only: there is no decision to take here, only a refresh.
                  <IconButton
                    icon={fetching ? Fetching : RefreshCw}
                    size="sm"
                    label="Fetch all remotes"
                    onClick={() => void refresh()}
                    disabled={fetching}
                    data-testid="run-source-fetch"
                  />
                )}
              </>
            ) : (
              // The reason is a sentence: in the tooltip, the row stays one dash.
              <span className="ei-meta" title={drift.reason} data-testid="run-source-unavailable">
                —
              </span>
            ),
          },
        ]}
      />
    </div>
  );
}

/**
 * The **Source** rows of the Info tab (#803, ADR-0070 §4): where the Run was cut
 * from, its fork point, and how far the two have drifted apart since.
 *
 * The fetch button is the ONLY gesture here, and it is explicit: displaying a Run
 * never touches the network (ADR-0070 §1). It reuses the #802 verb, so a fetch run
 * from a Run refreshes exactly what the next launch form will read — same repository.
 * An archived Run whose branch was cleaned up loses it: there is nothing to refresh.
 */
