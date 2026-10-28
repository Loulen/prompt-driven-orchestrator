import { RotateCcw, Square } from "lucide-react";
import { Button } from "../../../shared/ui/kit";
import NodeBanner from "./NodeBanner";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import AwaitingBanner from "./AwaitingBanner";
import { isOutputValidationFailure } from "../../model/nodeStatus";
import type { NodeGestureRights } from "./types";

// The prefix the daemon puts on a merge-back conflict's interruption reason
// (`merge_conflict: <detail>`, #601).
const MERGE_CONFLICT_PREFIX = "merge_conflict: ";

/**
 * The Interrupted banner's fact and detail. A merge-back conflict is not a dead session:
 * since ADR-0082 no resolver takes it over, so the node parks here with its work
 * in its sub-worktree, and « Session died » sent the reader looking for a crash.
 */
function interruptedBanner(reason: string | null | undefined): { title: string; text: string } {
  if (reason?.startsWith(MERGE_CONFLICT_PREFIX)) {
    const detail = reason.slice(MERGE_CONFLICT_PREFIX.length);
    return { title: "Merge-back conflict", text: `${detail} · work kept in the node's worktree` };
  }
  return { title: "Session died", text: `${reason ? `${reason} · ` : ""}work presumed intact` };
}

/**
 * The status banners stacked between the header and the terminal, one tinted
 * line each: completion released, awaiting user, stale, stopped, interrupted,
 * skipped, frontmatter retry and the two failed variants.
 */
export default function NodeBanners({
  node,
  isArchived,
  selectedIter,
  selectedIterStatus,
  completionReleased,
  nodeAwaitsOnLatestIter,
  terminalLive,
  terminalSpectating,
  canReleaseCompletion,
  isReleasing,
  releaseCompletion,
  markComplete,
  onOpenChildRun,
  killStale,
  restartIteration,
  retry,
  may,
}: {
  node: NodeState;
  isArchived?: boolean;
  selectedIter: number;
  selectedIterStatus: NodeStatus;
  completionReleased: boolean;
  nodeAwaitsOnLatestIter: boolean;
  terminalLive: boolean | null;
  terminalSpectating: boolean;
  canReleaseCompletion: boolean;
  isReleasing: boolean;
  releaseCompletion: () => void;
  markComplete: () => void;
  onOpenChildRun?: (childRunId: string) => void;
  killStale: () => void;
  restartIteration: () => void;
  retry: () => void;
  /** CORE-42: the gestures on this node the caller may make; the others hide. */
  may: NodeGestureRights;
}) {
  // A failed node is retried from its banner too.
  const failedActions = !isArchived && may.retry && (
    <Button size="sm" icon={RotateCcw} data-testid="failed-retry-btn" onClick={retry}>
      Retry
    </Button>
  );
  return (
    <>
      {/* #588 / ADR-0069 — the declared wait, on the iteration on screen. Only
          the latest iteration carries `node.awaiting`; an older awaiting
          iteration (a reaped one) renders the bare shape. A *derived* wait (a
          child awaiting) is a read-time overlay on the node rollup: the latest
          iteration is trusted through `node.awaiting` as well, so the banner
          cannot vanish when the two disagree. */}
      {(selectedIterStatus === "awaiting_user" || nodeAwaitsOnLatestIter) &&
        !completionReleased && (
        <AwaitingBanner
          awaiting={selectedIter === node.iter ? node.awaiting : null}
          terminalLive={terminalLive ?? true}
          terminalSpectating={terminalSpectating}
          canRelease={canReleaseCompletion && !isReleasing}
          canMarkComplete={may.markComplete}
          onRelease={releaseCompletion}
          onMarkComplete={markComplete}
          onOpenChildRun={onOpenChildRun}
        />
      )}

      {/* Stale banner — HISTORICAL RUNS ONLY since #469 (ADR-0032 §1).
          Nothing in the daemon marks a live node `stale` any more: the idle
          threshold that produced it killed healthy agents mid `docker build`. This
          block is therefore dead code for every new Run, and kept because Runs
          recorded before #469 still project `stale` and must still render as they
          always did. Do NOT "clean it up", and do not wire a producer back in to
          give it something to show. */}
      {node.status === "stale" && (
        <NodeBanner
          tone="skipped"
          title="Idle 2m+"
          text="outputs incomplete"
          testId="stale-banner"
          actions={
            !isArchived && (
              <>
                {may.kill && (
                  <Button size="sm" icon={Square} data-testid="stale-stop-btn" onClick={killStale}>
                    Stop
                  </Button>
                )}
                {may.restart && (
                  <Button size="sm" icon={RotateCcw} data-testid="stale-retry-btn" onClick={restartIteration}>
                    Retry
                  </Button>
                )}
              </>
            )
          }
        />
      )}

      {node.status === "stopped" && (
        <NodeBanner tone="skipped" title="Stopped" text={node.failure_reason ?? undefined} testId="stopped-banner" />
      )}

      {/* Interrupted banner — #598 / ADR-0049. An infra incident killed the
          session (tmux gone, daemon restart, spawn-abort); the work on disk is
          presumed intact and the run is parked, not failed. The one action that
          matters here is Reopen: it re-drives the interrupted node (the daemon
          reopens the run atomically on the node retry). */}
      {node.status === "interrupted" && (
        <NodeBanner
          tone="awaiting"
          testId="interrupted-banner"
          {...interruptedBanner(node.failure_reason)}
          actions={
            !isArchived &&
            may.retry && (
              <Button size="sm" icon={RotateCcw} data-testid="interrupted-reopen-btn" onClick={retry}>
                Reopen
              </Button>
            )
          }
        />
      )}

      {/* Skipped banner (#620) — a structurally-unreachable node the resilience
          sweep pruned. It never ran, its branch was not taken. The reason reads
          at node level (skip_reason), lifted from the skip event. */}
      {node.status === "skipped" && (
        <NodeBanner tone="skipped" title="Skipped" text={node.skip_reason ?? "branch not taken"} testId="skipped-banner" />
      )}

      {node.status === "running" && (node.frontmatter_retries ?? 0) > 0 && (
        <NodeBanner
          tone="awaiting"
          title="Frontmatter mismatch"
          text="corrective message sent, awaiting retry"
          testId="frontmatter-retry-banner"
        />
      )}

      {/* Failed banner — output-validation variant (#490): the reason
          VERBATIM, then the evidence, one line each. */}
      {node.status === "failed" && isOutputValidationFailure(node) && (
        <NodeBanner
          tone="failed"
          title="Failed"
          text={node.failure_reason ?? undefined}
          testId="output-validation-banner"
          actions={failedActions}
        >
          {node.frontmatter_violations && node.frontmatter_violations.length > 0 && (
            <ul className="ri-banner__list" data-testid="frontmatter-violation-list">
              {node.frontmatter_violations.map((v, i) => (
                <li key={i}>
                  {v.port}.{v.field}: {v.reason}
                </li>
              ))}
            </ul>
          )}
          {/* #490: a `script` node failing on a MISSING output used to paint this
              banner with an empty list. */}
          {node.missing_outputs && node.missing_outputs.length > 0 && (
            <ul className="ri-banner__list" data-testid="missing-output-list">
              {node.missing_outputs.map((m) => (
                <li key={m}>Missing outputs: {m}</li>
              ))}
            </ul>
          )}
        </NodeBanner>
      )}

      {/* Failed banner — generic. Same predicate, inverted: two different tests
          here would let both banners fire (or neither). */}
      {node.status === "failed" && !isOutputValidationFailure(node) && (
        <NodeBanner
          tone="failed"
          title="Failed"
          text={node.failure_reason ?? undefined}
          testId="failed-banner"
          actions={failedActions}
        />
      )}
    </>
  );
}
