import { Play, RefreshCw, RotateCcw, Square } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import { IconButton, Tooltip } from "../../../shared/ui/kit";
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

/**
 * The node's lifecycle controls, one line: Review, Retry, Start, Restart clean
 * and Stop as icons — a disabled one stays in place. The completion gesture,
 * Approve ▾, lives at the right of the detail tabs (`ApproveButton`). A
 * gesture the caller may not make stays, greyed, its tooltip saying why
 * (CORE-205).
 */
export default function NodeControls({
  node,
  stop,
  restartIteration,
  start,
  retry,
  refused,
  review,
}: {
  node: NodeState;
  stop: () => void;
  restartIteration: () => void;
  start: () => void;
  retry: () => void;
  /** CORE-205: the gestures the caller may not make, and why. */
  refused: NodeGestureRefusals;
  /** #749: the Review page with this node's delivery preselected, and its name. */
  review?: { href: string; label: string } | null;
}) {
  const s = node.status;
  const showRetry = RETRYABLE.has(s);
  const showStart = s === "pending";
  const showRestart = s === "running" && node.isolated_worktree === true;
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
      <IconButton
        icon={Square}
        label="Stop"
        tone="danger"
        data-testid="stop-btn"
        disabled={s !== "running" && !refused.stop}
        disabledReason={refused.stop}
        onClick={s === "running" ? stop : undefined}
      />
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
