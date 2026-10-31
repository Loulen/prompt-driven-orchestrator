import { Check, CheckCheck, CircleCheck, LockOpen, Play, RefreshCw, RotateCcw, Square } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import { Button, IconButton, Tooltip } from "../../../shared/ui/kit";
import { ReviewIcon } from "../../../shared/ui/brand/AccentIcons";
import type { ActionVerdict } from "../../store/useNodeRun";
import NodeBanner from "./NodeBanner";
import type { NodeGestureRights } from "./types";

/** The statuses a Retry re-drives (#598, ADR-0049, ADR-0086), with a live one. */
const RETRYABLE: ReadonlySet<NodeStatus> = new Set([
  "running",
  "completed",
  "failed",
  "stopped",
  "stale",
  "interrupted",
  "skipped",
]);

/** The statuses a completion may be released on, or a node marked complete. */
const COMPLETABLE: ReadonlySet<NodeStatus> = new Set(["awaiting_user", "running", "failed", "stale", "interrupted"]);

export interface NodeCompletionControls {
  /** Whether the caller may release the completion of this interactive iteration. */
  canRelease: boolean;
  released: boolean;
  isReleasing: boolean;
  /** #588 (A3): a refused unreleased completion makes « Mark ready » the call to action. */
  releaseIsPrimary: boolean;
  release: () => void;
  /** CORE-42: « Mark complete » is allowed to the caller, and the daemon would complete the node (else hidden). */
  canMarkComplete: boolean;
  markComplete: () => void;
}

/**
 * The node's controls, one line: Retry, Start, Restart clean, Mark ready,
 * Mark complete and Stop as icons — a disabled one stays in place — and
 * Approve as the ruby button while the node awaits you. A gesture the caller
 * may not make is not shown (CORE-42); without any, nothing.
 */
export default function NodeControls({
  node,
  stop,
  restartIteration,
  start,
  retry,
  may,
  completion,
  review,
}: {
  node: NodeState;
  stop: () => void;
  restartIteration: () => void;
  start: () => void;
  retry: () => void;
  may: NodeGestureRights;
  /** Absent on an archived Run: nothing completes there. */
  completion?: NodeCompletionControls;
  /** #749: the Review page with this node's delivery preselected, and its name. */
  review?: { href: string; label: string } | null;
}) {
  const s = node.status;
  const showRetry = may.retry && RETRYABLE.has(s);
  const showStart = may.start && s === "pending";
  const showRestart = may.restart && s === "running" && node.isolated_worktree === true;
  const showRelease = completion?.canRelease === true && COMPLETABLE.has(s);
  const showComplete = completion?.canMarkComplete === true && COMPLETABLE.has(s);
  const approve = showComplete && s === "awaiting_user";
  if (!showRetry && !showStart && !showRestart && !may.stop && !showRelease && !showComplete && !review) {
    return null;
  }
  const ReleaseIcon = completion?.released ? CircleCheck : LockOpen;
  const releaseLabel = completion?.isReleasing
    ? "Releasing…"
    : completion?.released
      ? "Completion released"
      : "Mark ready for completion";
  return (
    <div className="ri-ctl" role="toolbar" aria-label="Node actions" data-testid="node-controls">
      {review && (
        <Tooltip label={review.label}>
          <a href={review.href} className="d-icon" aria-label={review.label} data-testid="node-review-shortcut">
            <ReviewIcon size={14} />
          </a>
        </Tooltip>
      )}
      {showRetry && (
        <IconButton
          icon={RotateCcw}
          label="Retry"
          // A live node's retry, and a settled node's (the old « Play »).
          data-testid={s === "running" ? "retry-btn" : "play-retry-btn"}
          onClick={retry}
        />
      )}
      {showStart && <IconButton icon={Play} label="Start" data-testid="start-btn" onClick={start} />}
      {showRestart && (
        <IconButton icon={RefreshCw} label="Restart clean" data-testid="restart-iteration-btn" onClick={restartIteration} />
      )}
      {showRelease &&
        !completion.releaseIsPrimary && (
          <IconButton
            icon={ReleaseIcon}
            label={releaseLabel}
            pressed={completion.released}
            disabled={completion.isReleasing}
            data-testid="release-completion-btn"
            onClick={completion.release}
          />
        )}
      {showComplete && !approve && (
        <IconButton icon={CheckCheck} label="Mark complete" data-testid="mark-complete-btn" onClick={completion.markComplete} />
      )}
      {may.stop && (
        <IconButton
          icon={Square}
          label="Stop"
          tone="danger"
          data-testid="stop-btn"
          disabled={s !== "running"}
          onClick={s === "running" ? stop : undefined}
        />
      )}
      {showRelease && completion.releaseIsPrimary && (
        <Button
          tone="primary"
          size="sm"
          icon={LockOpen}
          data-testid="release-completion-btn"
          data-primary
          disabled={completion.isReleasing}
          onClick={completion.release}
        >
          {completion.isReleasing ? "Releasing…" : "Mark ready"}
        </Button>
      )}
      {approve && (
        <Button
          tone="primary"
          size="sm"
          icon={Check}
          data-testid="mark-complete-btn"
          title="Approve: take the artifacts as they are"
          onClick={completion.markComplete}
        >
          Approve
        </Button>
      )}
    </div>
  );
}

/** #487: the refusal of a Retry/Play or Start click, rendered AT the gesture. */
export function ActionVerdictBanner({
  actionVerdict,
}: {
  actionVerdict: ActionVerdict;
}) {
  const title =
    actionVerdict.action === "retry"
      ? "Retry refused"
      : actionVerdict.action === "restart"
        ? "Restart refused"
        : "Start refused";
  return (
    <NodeBanner
      tone="failed"
      title={title}
      text={actionVerdict.message}
      testId="action-verdict"
      data={{ "data-action": actionVerdict.action }}
    />
  );
}
