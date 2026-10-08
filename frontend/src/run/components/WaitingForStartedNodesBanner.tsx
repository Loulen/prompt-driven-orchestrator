import { Loader } from "lucide-react";
import { waitingForStartedNodesLabel } from "../model/nodePlay";

/**
 * ADR-0086 « Attente des nœuds lancés »: the Run reached `End` but still runs,
 * because nodes it launched are not done yet. It ends by itself when the last
 * of them completes.
 */
export default function WaitingForStartedNodesBanner({ count }: { count: number }) {
  return (
    <div
      className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-1.5"
      data-testid="run-waiting-started-nodes"
      role="status"
    >
      <Loader size={12} className="shrink-0 animate-spin text-st-await" />
      <span className="text-st-await" style={{ fontSize: "11.5px", fontWeight: 500 }}>
        Reached End, {waitingForStartedNodesLabel(count)}
      </span>
      <span className="text-fg-3" style={{ fontSize: "11px" }}>
        — the run ends when they are done.
      </span>
    </div>
  );
}
