import { AlertCircle, Play, RotateCcw, Square } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { ActionVerdict } from "../../store/useNodeRun";
import RetryPlayButton from "./RetryPlayButton";
import { RETRY_BUTTON_CLASS, RETRY_BUTTON_STYLE } from "./labels";

/** The Stop / Restart / Start / Retry-Play toolbar of a live (non-archived) run. */
export default function NodeControls({
  node,
  stop,
  restartIteration,
  start,
  retry,
  canStart = true,
  canRetry = true,
}: {
  node: NodeState;
  stop: () => void;
  restartIteration: () => void;
  start: () => void;
  retry: () => void;
  /** ADR-0086: the Autorisation d'opération would allow the Start. */
  canStart?: boolean;
  /** ADR-0086: the Autorisation d'opération would allow the Retry. */
  canRetry?: boolean;
}) {
  return (
    <div
      className="flex items-center gap-1.5 border-b border-line px-3 py-1.5"
      data-testid="node-controls"
    >
      <button
        data-testid="stop-btn"
        disabled={node.status !== "running"}
        onClick={node.status === "running" ? stop : undefined}
        className={
          node.status === "running"
            ? "flex cursor-pointer items-center gap-1 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-0.5 text-st-failed transition-colors hover:bg-st-failed/20"
            : "flex items-center gap-1 rounded border border-line bg-bg-3 px-2 py-0.5 text-fg-4 opacity-50"
        }
        style={{ fontSize: "10.5px", fontWeight: 500 }}
      >
        <Square size={10} />
        Stop
      </button>
      {node.status === "running" && node.isolated_worktree && (
        <button
          data-testid="restart-iteration-btn"
          onClick={restartIteration}
          className={RETRY_BUTTON_CLASS}
          style={RETRY_BUTTON_STYLE}
        >
          <RotateCcw size={10} />
          Restart
        </button>
      )}
      {node.status === "pending" && canStart && (
        <button
          data-testid="start-btn"
          onClick={start}
          className={RETRY_BUTTON_CLASS}
          style={RETRY_BUTTON_STYLE}
        >
          <Play size={10} />
          Start
        </button>
      )}
      {canRetry && <RetryPlayButton status={node.status} onClick={retry} />}
    </div>
  );
}

/** #487: the refusal of a Retry/Play or Start click, rendered AT the gesture. */
export function ActionVerdictBanner({
  actionVerdict,
}: {
  actionVerdict: ActionVerdict;
}) {
  return (
    <div
      className="flex items-start gap-2 border-b border-st-failed/30 bg-st-failed/10 px-3 py-2"
      data-testid="action-verdict"
      data-action={actionVerdict.action}
    >
      <AlertCircle size={14} className="mt-0.5 shrink-0 text-st-failed" />
      <span className="text-st-failed" style={{ fontSize: "11.5px", fontWeight: 500 }}>
        {actionVerdict.action === "retry"
          ? "Retry refused"
          : actionVerdict.action === "restart"
            ? "Restart refused"
            : "Start refused"}{" "}
        —{" "}
        {actionVerdict.message}
      </span>
    </div>
  );
}
