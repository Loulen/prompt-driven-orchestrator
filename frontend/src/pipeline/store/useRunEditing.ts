/**
 * A Run's canvas in « pilotage » or in « Edit for this run » (ADR-0085): whether
 * it is read-only, the toolbar's Edit control, and the confirmation « Finish
 * editing » asks for when edits are unsaved.
 */

import { useCallback, useState } from "react";
import type { RunState } from "../../shared/generated/RunState";
import type { RunEdit } from "../components/RunEditControl";
import { hasUnsavedWork, isRunTabLocked, type OpenPipeline } from "./editState";
import { useEditStore } from "./editStore";

/** The three ways out of the confirmation, `null` while it is closed. */
export interface FinishEditingConfirm {
  onCancel: () => void;
  onDiscard: () => void;
  onSave: () => void;
}

/** The shared pipeline a Run was launched from: the id its launch carried, else its name. */
export function sourcePipelineId(run: RunState): string {
  return run.pipeline_id || run.pipeline_name;
}

export function useRunEditing(tab: OpenPipeline | undefined, activeRunState: RunState | null) {
  const startRunEditing = useEditStore((s) => s.startRunEditing);
  const finishRunEditing = useEditStore((s) => s.finishRunEditing);
  const saveTab = useEditStore((s) => s.save);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const [confirming, setConfirming] = useState(false);

  // #315 / ADR-0020: an archived Run's canvas is read-only for good (its
  // worktree and pipeline.yaml are gone). ADR-0085: every Run tab is read-only
  // the same way while followed; « Edit for this run » unlocks it.
  const archived = activeRunState?.status === "archived";
  const readOnly = archived || isRunTabLocked(tab);

  const handleFinishEditing = useCallback(() => {
    if (!tab) return;
    if (hasUnsavedWork(tab)) {
      setConfirming(true);
      return;
    }
    void finishRunEditing(tab.id);
  }, [tab, finishRunEditing]);

  const runEdit: RunEdit | undefined =
    tab?.runId != null && activeRunState != null && !archived
      ? {
          editing: !isRunTabLocked(tab),
          onEditForRun: () => startRunEditing(tab.id),
          onEditSource: () => void openPipeline(sourcePipelineId(activeRunState)),
          onFinishEditing: handleFinishEditing,
        }
      : undefined;

  const finishConfirm: FinishEditingConfirm | null =
    confirming && tab
      ? {
          onCancel: () => setConfirming(false),
          onDiscard: () => {
            setConfirming(false);
            void finishRunEditing(tab.id);
          },
          onSave: async () => {
            setConfirming(false);
            await saveTab(tab.id);
            const after = useEditStore.getState().openTabs.find((t) => t.id === tab.id);
            // A failed save keeps the edits, and the save-error modal says why.
            if (after && !hasUnsavedWork(after)) await finishRunEditing(tab.id);
          },
        }
      : null;

  return { readOnly, runEdit, finishConfirm };
}
