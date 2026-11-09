// What the Run pane of the inspector shows before a node of the Run is live.
import { Hourglass } from "lucide-react";
import { EmptyState } from "../shared/ui/kit";

export function RunTabPlaceholder({ nodeId }: { nodeId: string | null }) {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-surface-1"
      data-testid="pending-placeholder"
    >
      <EmptyState icon={Hourglass}>
        <em>Not started yet</em>
      </EmptyState>
      {nodeId && <div className="d-mono mt-1 text-xs text-text-3">{nodeId}</div>}
    </aside>
  );
}
