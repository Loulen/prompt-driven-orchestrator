import { LoaderCircle } from "lucide-react";
import type { RunState } from "../../shared/generated/RunState";
import { waitingForStartedNodesLabel } from "../model/nodePlay";
import type { useNodePlay } from "../store/useNodePlay";
import RetryConfirmDialog from "./nodeDetail/RetryConfirmDialog";

/**
 * ADR-0086 « Attente des nœuds lancés »: the Run reached `End` but still runs,
 * because nodes it launched are not done yet. It ends by itself when the last
 * of them completes.
 */
export default function WaitingForStartedNodesBanner({ run }: { run: RunState | null }) {
  const count = run?.waiting_for_started_nodes ?? 0;
  if (count === 0) return null;
  return (
    <div
      className="cv-line"
      data-st="awaiting"
      data-testid="run-waiting-started-nodes"
      role="status"
      title="The run ends when they are done"
    >
      <LoaderCircle size={13} strokeWidth={1.5} className="d-spin" aria-hidden />
      <span className="cv-line__msg">Reached End, {waitingForStartedNodesLabel(count)}</span>
    </div>
  );
}

/** The same wait, as the Info tab of a Run says it. */
export function WaitingForStartedNodesReason({ run }: { run: RunState | null }) {
  const count = run?.waiting_for_started_nodes ?? 0;
  if (count === 0) return null;
  return (
    <div
      className="mt-2 rounded border border-st-await/30 bg-st-await/14 px-2 py-1.5 text-xs text-text-2"
      data-testid="run-waiting-started-nodes-reason"
    >
      <div className="font-medium text-st-await">Reached End, {waitingForStartedNodesLabel(count)}</div>
      <div className="mt-0.5 break-words">The run ends by itself when they are done.</div>
    </div>
  );
}

/** The « Retry this node? » confirmation of a card's ▶, while one is pending. */
export function NodePlayRetryConfirm({ nodePlay }: { nodePlay: ReturnType<typeof useNodePlay> }) {
  if (!nodePlay.retryConfirm) return null;
  return (
    <RetryConfirmDialog
      retryConfirm={nodePlay.retryConfirm}
      confirmRetry={() => void nodePlay.confirmRetry()}
      cancelRetry={nodePlay.cancelRetry}
    />
  );
}
