import { useCallback, useEffect, useState } from "react";
import type { RunState } from "../shared/generated/RunState";
import { useEditStore } from "../pipeline/store/editStore";
import { useMountedPages } from "../platform/store/mountedPages";
import { childLandingNode } from "../run/model/orchestration";

/** What a navigation to a Run clears of the shell's other selections. */
export interface RunNavigationTargets {
  selectedRun: RunState | null;
  selectRun: (runId: string | null) => void;
  setSelectedTriggerId: (triggerId: string | null) => void;
  setSelectedRunId: (runId: string | null) => void;
  setSelectedNodeId: (nodeId: string | null) => void;
}

/**
 * Selecting a Run, and the way back from a child run opened in an Orchestration
 * tab (#723).
 */
export function useRunNavigation({
  selectedRun,
  selectRun,
  setSelectedTriggerId,
  setSelectedRunId,
  setSelectedNodeId,
}: RunNavigationTargets) {
  const openRunPipeline = useEditStore((s) => s.openRunPipeline);
  const selection = useEditStore((s) => s.selection);
  const setSelection = useEditStore((s) => s.setSelection);
  // #723 — the way back from a child run opened in an Orchestration tab: parent
  // run + orchestrator node. Declared before `handleSelectRun`, which clears
  // both on any navigation — the bar's visibility is derived (child run
  // selected), so no effect ever needs to reconcile them.
  const [orchestratorReturn, setOrchestratorReturn] = useState<{
    parentRunId: string;
    parentRunName: string;
    nodeId: string;
    nodeName: string;
    childRunId: string;
  } | null>(null);
  // One-shot landing: set by the back gesture, consumed by the remount of the
  // orchestrator's NodeDetailPanel (I/O stays the default everywhere else).
  const [landOnOrchestration, setLandOnOrchestration] = useState<string | null>(null);
  // One-shot landing the other way: the child run just opened, whose node to
  // select once its state has arrived.
  const [landOnChild, setLandOnChild] = useState<string | null>(null);

  const handleSelectRun = useCallback(
    async (runId: string) => {
      // #723: any navigation away from the child run ends the back-bar story
      // and any pending Orchestration-tab landing — both are one gesture old.
      // (Purely derived visibility does the rest: the bar only ever renders
      // while the child run is the selected one.)
      setOrchestratorReturn(null);
      setLandOnOrchestration(null);
      setLandOnChild(null);
      // A Run takes the center back from a mounted page's tab (which stays open).
      useMountedPages.getState().showPage(null);
      setSelectedTriggerId(null);
      setSelectedRunId(runId);
      selectRun(runId);
      setSelectedNodeId(null);
      await openRunPipeline(runId);
    },
    [
      selectRun,
      openRunPipeline,
      setOrchestratorReturn,
      setLandOnOrchestration,
      setSelectedTriggerId,
      setSelectedRunId,
      setSelectedNodeId,
    ],
  );

  // #723 — opening a child run from the Orchestration tab: record the way back
  // (parent run + orchestrator node) AFTER the navigation, so the shared
  // `handleSelectRun`'s clear does not wipe it.
  const handleOpenChildRun = useCallback(
    async (childRunId: string) => {
      if (!selectedRun || selection.kind !== "node" || !selection.id) return;
      const entry = {
        parentRunId: selectedRun.run_id,
        parentRunName: selectedRun.name || selectedRun.pipeline_name,
        nodeId: selection.id,
        nodeName:
          selectedRun.node_defs?.find((d) => d.id === selection.id)?.name ?? selection.id,
        childRunId,
      };
      await handleSelectRun(childRunId);
      setOrchestratorReturn(entry);
      setLandOnChild(childRunId);
    },
    [selectedRun, selection, handleSelectRun, setOrchestratorReturn],
  );
  // The child run opens on the node that needs a look (its banner at once),
  // as soon as its state is the selected one; a selection made meanwhile wins.
  useEffect(() => {
    if (!landOnChild || selectedRun?.run_id !== landOnChild) return;
    setLandOnChild(null);
    const nodeId = childLandingNode(selectedRun);
    if (nodeId && selection.kind === "none") setSelection({ kind: "node", id: nodeId });
  }, [landOnChild, selectedRun, selection.kind, setSelection]);
  const handleBackToOrchestrator = useCallback(async () => {
    if (!orchestratorReturn) return;
    const { parentRunId, nodeId } = orchestratorReturn;
    await handleSelectRun(parentRunId);
    // One-shot landing: the panel remounts on this selection and seeds its
    // detail tab from it — two gestures deep, two gestures back.
    setLandOnOrchestration(nodeId);
    setSelection({ kind: "node", id: nodeId });
  }, [orchestratorReturn, handleSelectRun, setSelection, setLandOnOrchestration]);

  return {
    orchestratorReturn,
    landOnOrchestration,
    handleSelectRun,
    handleOpenChildRun,
    handleBackToOrchestrator,
  };
}
