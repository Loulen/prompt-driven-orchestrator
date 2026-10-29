import { useCallback, useState } from "react";
import { RUN_INTENT, type OpenIntent } from "../run/components/NewRunModal";
import type { Trigger } from "../shared/generated/Trigger";
import { useEditStore } from "../pipeline/store/editStore";
import { showToast } from "../shared/ui/toast";

/** What selecting a Trigger clears and opens. */
export interface LaunchTargets {
  triggers: Trigger[];
  triggersPaused: boolean;
  selectRun: (runId: string | null) => void;
  setSelectedTriggerId: (triggerId: string | null) => void;
  setSelectedRunId: (runId: string | null) => void;
  setSelectedNodeId: (nodeId: string | null) => void;
  setTriggerOpenedTabId: (tabId: string | null) => void;
}

/** The New Run modal (a Run, or a Trigger to create or edit), the selection of
 *  a Trigger, and its manual « Run now » (#341). */
export function useLaunchActions({
  triggers,
  triggersPaused,
  selectRun,
  setSelectedTriggerId,
  setSelectedRunId,
  setSelectedNodeId,
  setTriggerOpenedTabId,
}: LaunchTargets) {
  const openPipeline = useEditStore((s) => s.openPipeline);
  const [newRunModalOpen, setNewRunModalOpen] = useState(false);
  // #386: how the always-mounted New Run modal should open. Drives a one-shot
  // reset on every reopen so a dismissed "Edit trigger" can't leak into a fresh
  // "New run" / "New trigger". Defaults to a plain run.
  const [openIntent, setOpenIntent] = useState<OpenIntent>(RUN_INTENT);

  const openNewRunModal = useCallback((intent: OpenIntent) => {
    setOpenIntent(intent);
    setNewRunModalOpen(true);
  }, []);

  const handleSelectTrigger = useCallback(
    (triggerId: string) => {
      // Selecting a Trigger clears the run/node selection so the detail panel
      // wins the right pane.
      setSelectedTriggerId(triggerId);
      setSelectedRunId(null);
      setSelectedNodeId(null);
      selectRun(null);
      // #320: also open the instance-owned pipeline this Trigger would launch.
      const trig = triggers.find((t) => t.id === triggerId);
      if (trig) {
        setTriggerOpenedTabId(trig.pipeline_id);
        openPipeline(trig.pipeline_id);
      }
    },
    [selectRun, triggers, openPipeline, setSelectedTriggerId, setSelectedRunId, setSelectedNodeId, setTriggerOpenedTabId],
  );

  // #341: "Run now" is a real fire (guard + overlap + audit row), not a
  // prefilled New Run modal. Fire, then open the trigger detail (via
  // handleSelectTrigger — the only path that survives the #320 reconciliation)
  // where the new history row appears. A 409 (disabled / broken reference)
  // surfaces as an error toast.
  const fireNow = useCallback(
    async (t: Trigger) => {
      try {
        const { fireTrigger } = await import("../trigger/api/triggers");
        await fireTrigger(t.id);
      } catch (e) {
        showToast({ kind: "error", title: "Run now", message: e instanceof Error ? e.message : String(e) });
      }
      handleSelectTrigger(t.id);
    },
    [handleSelectTrigger],
  );

  // #348 (D3-A permissive): a manual fire still works while triggers are
  // globally paused — but behind a confirmation (`pausedFire`), so it stays a
  // deliberate escape hatch rather than a silent bypass of the kill-switch.
  const [pausedFire, setPausedFire] = useState<Trigger | null>(null);
  const handleRunNowTrigger = useCallback(
    async (t: Trigger) => {
      if (triggersPaused) {
        setPausedFire(t);
        return;
      }
      await fireNow(t);
    },
    [fireNow, triggersPaused],
  );
  const confirmPausedFire = useCallback(() => {
    const t = pausedFire;
    setPausedFire(null);
    if (t) void fireNow(t);
  }, [pausedFire, fireNow]);
  const cancelPausedFire = useCallback(() => setPausedFire(null), []);

  const handleCloseNewRunModal = useCallback(() => {
    setNewRunModalOpen(false);
    // Reset the intent to a plain run. Harmless while closed (the modal's reset
    // only fires on open false→true), but keeps the next default-less open clean.
    setOpenIntent(RUN_INTENT);
  }, []);

  return {
    newRunModalOpen,
    openIntent,
    openNewRunModal,
    handleSelectTrigger,
    handleRunNowTrigger,
    pausedFire,
    confirmPausedFire,
    cancelPausedFire,
    handleCloseNewRunModal,
  };
}
