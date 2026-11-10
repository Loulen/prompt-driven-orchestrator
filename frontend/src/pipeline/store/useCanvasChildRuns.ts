import { useCallback } from "react";
import type { Node } from "@xyflow/react";
import type { RunState } from "../../shared/generated/RunState";
import { childrenOfNode, countChildren, useRunChildren } from "../../run/model/orchestration";
import { useOrchestrationFocus } from "../../run/store/orchestrationFocus";
import { useEditStore } from "./editStore";

/**
 * #723 — the child runs of a Run, read with the view; `withChildren` hangs
 * them on the card of any parent node (ADR-0064: a node with at least one
 * child run, agent or script, Orchestrator toggle or not): their counts, and
 * the gesture that opens the node's Orchestration tab. A node without a child
 * keeps its card as it was. Template tabs never read (no run state, no
 * children) and stay untouched.
 */
export function useCanvasChildRuns(runState: RunState | null, onCloseInfo?: () => void) {
  const runId = runState?.run_id ?? null;
  const runChildren = useRunChildren(runId, runState != null);
  const setSelection = useEditStore((s) => s.setSelection);
  const withChildren = useCallback(
    (card: Node): Node => {
      if (!runId) return card;
      const nodeChildren = childrenOfNode(runChildren, runId, card.id);
      if (nodeChildren.length === 0) return card;
      // Selects the node, as a click on its card does, then asks its panel for
      // the Orchestration tab.
      const onOpenOrchestration = () => {
        setSelection({ kind: "node", id: card.id });
        onCloseInfo?.();
        useOrchestrationFocus.getState().open(runId, card.id);
      };
      return { ...card, data: { ...card.data, childCounts: countChildren(nodeChildren), onOpenOrchestration } };
    },
    [runId, runChildren, setSelection, onCloseInfo],
  );
  return withChildren;
}
