import { CheckCircle, GitCompareArrows, LockOpen, LoaderCircle } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import { reviewUrl } from "../../model/runRefs";
import type { RefPair } from "../../model/runRefs";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";

/**
 * The actions at the top of the details pane: the Review shortcut, « Mark ready
 * for completion », « Mark complete » and the verdicts of those clicks.
 */
export default function NodeActions({
  node,
  runId,
  isArchived,
  reviewTarget,
  selectedIter,
  canReleaseCompletion,
  completionReleased,
  isReleasing,
  releaseIsPrimary,
  releaseVerdict,
  markVerdict,
  releaseCompletion,
  markComplete,
  canMarkComplete,
}: {
  node: NodeState;
  runId: string;
  isArchived?: boolean;
  reviewTarget: { pair: RefPair; label: string } | null;
  selectedIter: number;
  canReleaseCompletion: boolean;
  completionReleased: boolean;
  isReleasing: boolean;
  releaseIsPrimary: boolean;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
  releaseCompletion: () => void;
  markComplete: () => void;
  /** CORE-42: « Mark complete » is allowed to the caller (else hidden). */
  canMarkComplete: boolean;
}) {
  return (
    <div className="flex flex-col gap-1.5 px-3 py-2">
      {/* #749: the shortcut to the Review page with this node's delivery
          preselected (`before → after`, or `tip → live` while it runs
          isolated). There is no "node diff" surface (ADR-0067 §1): the
          panel only preselects a pair. Absent for a node that has
          nothing to review yet (pending, skipped, failed before
          delivering). */}
      {reviewTarget && !isArchived && (
        <a
          href={reviewUrl(runId, reviewTarget.pair)}
          data-testid="node-review-shortcut"
          className="flex w-full items-center justify-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:border-fg-4 hover:text-fg"
          style={{ fontSize: "11.5px", fontWeight: 500 }}
        >
          <GitCompareArrows size={12} />
          {reviewTarget.label}
        </a>
      )}
      {/* `interrupted` is included (#598 / ADR-0049): an interrupted
          interactive node parks the run `awaiting_user`, so the "take the
          artifacts as they are" escape must stay reachable — the daemon
          embeds the run reopen on `mark_node_done` too, and the output
          guard rejects gracefully (rendered at the gesture) if the work
          is genuinely incomplete. */}
      {(node.status === "awaiting_user" || node.status === "running" || node.status === "failed" || node.status === "stale" || node.status === "interrupted") && !isArchived && (
        <>
          <div className="flex flex-wrap gap-1.5">
            {canReleaseCompletion && (
                <button
                  onClick={releaseCompletion}
                  title="Enables the node to call pdo complete when it chooses and to proceed with the run"
                  disabled={isReleasing}
                  data-testid="release-completion-btn"
                  data-primary={releaseIsPrimary || undefined}
                  className={`flex min-w-[140px] flex-1 cursor-pointer items-center justify-center gap-1.5 rounded-md border border-st-await/40 bg-st-await-bg px-3 py-1.5 text-st-await transition-colors hover:border-st-await/60 hover:bg-st-await/20 disabled:cursor-wait disabled:opacity-70 ${
                    completionReleased ? "border-dashed" : ""
                  } ${releaseIsPrimary ? "ring-2 ring-st-await/50 ring-offset-1 ring-offset-bg" : ""}`}
                  style={{ fontSize: "11.5px", fontWeight: 500 }}
                >
                  {isReleasing ? (
                    <LoaderCircle size={12} className="animate-spin" />
                  ) : completionReleased ? (
                    <CheckCircle size={12} />
                  ) : (
                    <LockOpen size={12} />
                  )}
                  {isReleasing
                    ? "Releasing…"
                    : completionReleased
                      ? "Completion released"
                      : "Mark ready for completion"}
                </button>
            )}
              {canMarkComplete && (
              <button
                onClick={markComplete}
                title="Take the artifacts as they are and complete the node now"
                data-testid="mark-complete-btn"
                className="flex min-w-[140px] flex-1 cursor-pointer items-center justify-center gap-1.5 rounded-md border border-st-done/40 bg-st-done-bg px-3 py-1.5 text-st-done transition-colors hover:border-st-done/60 hover:bg-st-done/20"
                style={{ fontSize: "11.5px", fontWeight: 500 }}
              >
                <CheckCircle size={12} />
                Mark complete
              </button>
              )}
          </div>

          {completionReleased ? (
            <ReleaseCompletionVerdict verdict={{ kind: "released" }} />
          ) : (
            releaseVerdict &&
            releaseVerdict.iter === selectedIter && (
              <ReleaseCompletionVerdict verdict={releaseVerdict} />
            )
          )}

          {/* #490: the verdict of the click, AT the gesture. Rendered for
              every outcome including `pending`, so nothing ever blinks out
              without a replacement. */}
          {markVerdict && markVerdict.iter === selectedIter && (
            <MarkCompleteVerdict verdict={markVerdict} />
          )}
        </>
      )}
    </div>
  );
}
