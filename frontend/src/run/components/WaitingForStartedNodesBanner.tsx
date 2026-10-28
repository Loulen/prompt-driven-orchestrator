import { Loader } from "lucide-react";
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
    <div className="absolute left-0 right-0 top-10 z-10">
      <div
        className="flex items-center gap-2 border-b border-st-await/30 bg-st-await/14 px-3 py-1.5"
        data-testid="run-waiting-started-nodes"
        role="status"
      >
        <Loader size={12} className="shrink-0 animate-spin text-st-await" />
        <span className="text-st-await" style={{ fontSize: "11.5px", fontWeight: 500 }}>
          Reached End, {waitingForStartedNodesLabel(count)}
        </span>
        <span className="text-text-3" style={{ fontSize: "11px" }}>
          — the run ends when they are done.
        </span>
      </div>
    </div>
  );
}

/** The same wait, as the Info tab of a Run says it. */
export function WaitingForStartedNodesReason({ run }: { run: RunState | null }) {
  const count = run?.waiting_for_started_nodes ?? 0;
  if (count === 0) return null;
  return (
    <div
      className="mt-2 rounded border border-st-await/30 bg-st-await/14 px-2 py-1.5 text-text-2"
      style={{ fontSize: "10.5px" }}
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
