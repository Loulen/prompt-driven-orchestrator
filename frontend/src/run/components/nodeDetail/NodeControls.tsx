import { Check, CheckCheck, CircleCheck, LockOpen, Play, RefreshCw, RotateCcw, Square } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import { Button, IconButton, Tooltip } from "../../../shared/ui/kit";
import { ReviewIcon } from "../../../shared/ui/brand/AccentIcons";
import type { ActionVerdict } from "../../store/useNodeRun";
import NodeBanner from "./NodeBanner";
import type { NodeGestureRefusals } from "./types";

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
  /** Whether the completion of this interactive iteration may be released. */
  canRelease: boolean;
  /** CORE-205: why the caller may not release it (the gesture greyed), else absent. */
  releaseRefused?: string;
  released: boolean;
  isReleasing: boolean;
  /** #588 (A3): a refused unreleased completion makes « Mark ready » the call to action. */
  releaseIsPrimary: boolean;
  release: () => void;
  /** The daemon would complete the node (else hidden). */
  canMarkComplete: boolean;
  /** CORE-205: why the caller may not mark it complete (the gesture greyed), else absent. */
  markCompleteRefused?: string;
  markComplete: () => void;
}

/**
 * The node's controls, one line: Retry, Start, Restart clean, Mark ready,
 * Mark complete and Stop as icons — a disabled one stays in place — and
 * Approve as the ruby button while the node awaits you. A gesture the caller
 * may not make stays, greyed, its tooltip saying why (CORE-205); without any
 * gesture, nothing.
 */
export default function NodeControls({
  node,
  stop,
  restartIteration,
  start,
  retry,
  refused,
  completion,
  review,
}: {
  node: NodeState;
  stop: () => void;
  restartIteration: () => void;
  start: () => void;
  retry: () => void;
  /** CORE-205: the gestures the caller may not make, and why. */
  refused: NodeGestureRefusals;
  /** Absent on an archived Run: nothing completes there. */
  completion?: NodeCompletionControls;
  /** #749: the Review page with this node's delivery preselected, and its name. */
  review?: { href: string; label: string } | null;
}) {
  const s = node.status;
  const showRetry = RETRYABLE.has(s);
  const showStart = s === "pending";
  const showRestart = s === "running" && node.isolated_worktree === true;
  const showRelease = completion?.canRelease === true && COMPLETABLE.has(s);
  const showComplete = completion?.canMarkComplete === true && COMPLETABLE.has(s);
  const approve = showComplete && s === "awaiting_user";
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
          disabledReason={refused.retry}
          onClick={retry}
        />
      )}
      {showStart && (
        <IconButton icon={Play} label="Start" data-testid="start-btn" disabledReason={refused.start} onClick={start} />
      )}
      {showRestart && (
        <IconButton
          icon={RefreshCw}
          label="Restart clean"
          data-testid="restart-iteration-btn"
          disabledReason={refused.restart}
          onClick={restartIteration}
        />
      )}
      {showRelease &&
        !completion.releaseIsPrimary && (
          <IconButton
            icon={ReleaseIcon}
            label={releaseLabel}
            pressed={completion.released}
            disabled={completion.isReleasing}
            disabledReason={completion.releaseRefused}
            data-testid="release-completion-btn"
            onClick={completion.release}
          />
        )}
      {showComplete && !approve && (
        <IconButton
          icon={CheckCheck}
          label="Mark complete"
          data-testid="mark-complete-btn"
          disabledReason={completion.markCompleteRefused}
          onClick={completion.markComplete}
        />
      )}
      <IconButton
        icon={Square}
        label="Stop"
        tone="danger"
        data-testid="stop-btn"
        disabled={s !== "running" && !refused.stop}
        disabledReason={refused.stop}
        onClick={s === "running" ? stop : undefined}
      />
      {showRelease && completion.releaseIsPrimary && (
        <Button
          tone="primary"
          size="sm"
          icon={LockOpen}
          data-testid="release-completion-btn"
          data-primary
          disabled={completion.isReleasing}
          disabledReason={completion.releaseRefused}
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
          title={completion.markCompleteRefused ? undefined : "Approve: take the artifacts as they are"}
          disabledReason={completion.markCompleteRefused}
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
