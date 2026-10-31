/**
 * The parts of a work node's card that every card size shares: the header (the
 * type icon in the Theme's accent, the name, the flags, the status at the end)
 * and an orchestrator's « N runs » row with the Orchestration tab's pastilles.
 */
import type { ReactNode } from "react";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import { StatusIcon } from "../../shared/ui/kit";
import { ChildCountPills } from "../../run/components/OrchestrationTab";
import { totalChildren, type ChildCounts } from "../../run/model/orchestration";

/**
 * The header of a card: a bar tinted by the node's status (neutral on a
 * template, which has none), the type icon first, the name, the flags, then
 * the status icon on a Run.
 */
export function NodeCardHead({
  type,
  name,
  flags,
  status,
}: {
  /** The type icon, its markers included; `.cv-n__type` paints it in `--primary`. */
  type: ReactNode;
  name: string;
  flags?: ReactNode;
  /** The node's status in a Run; `null` on a template. */
  status: NodeStatus | null;
}) {
  return (
    <div className="cv-n__head" data-testid="node-card-head">
      <span className="cv-n__lead">{type}</span>
      <span className="cv-n__name" title={name}>
        {name}
      </span>
      {flags}
      {status && <StatusIcon kind="node" status={status} size={13} />}
    </div>
  );
}

/**
 * « N runs », then the pastilles of the Orchestration tab's header: one row that
 * opens the tab. Nothing while the node has no child.
 */
export function OrchestratorRuns({ counts, onOpen }: { counts: ChildCounts; onOpen?: () => void }) {
  const total = totalChildren(counts);
  if (total === 0) return null;
  return (
    <button
      type="button"
      data-testid="node-child-runs"
      className="nodrag cv-n__runs"
      onClick={(e) => {
        e.stopPropagation();
        onOpen?.();
      }}
    >
      <span className="cv-n__runs-n">
        {total} run{total === 1 ? "" : "s"}
      </span>
      <ChildCountPills counts={counts} size="pill" testId="node-child-pills" />
    </button>
  );
}
