/**
 * `raise cap` (ADR draft plafond-de-cout-de-boucle): a loop that reached its
 * Plafond de coût is edited « for this run ». The Run's tab unlocks (ADR-0085)
 * from the caps the Run applies, and the region inspector opens on
 * `max_cost_usd`, focused. Both updates land in one render, so the inspector
 * mounts editable and the focus takes.
 */

import { useCallback } from "react";
import type { CostCapOverrides } from "../model/runCostCaps";
import { useEditStore } from "./editStore";

export function useRaiseCap(): (
  runId: string,
  regionId: string,
  capOverrides: CostCapOverrides | null | undefined,
) => void {
  const startRunEditing = useEditStore((s) => s.startRunEditing);
  const setActiveTab = useEditStore((s) => s.setActiveTab);
  const setSelection = useEditStore((s) => s.setSelection);
  return useCallback(
    (runId, regionId, capOverrides) => {
      const { openTabs, activeTabId } = useEditStore.getState();
      // The Run's tab, brought to the front when another one is.
      const tab =
        openTabs.find((t) => t.id === activeTabId && t.runId === runId) ?? openTabs.find((t) => t.runId === runId);
      if (tab) {
        if (tab.id !== activeTabId) setActiveTab(tab.id);
        startRunEditing(tab.id, capOverrides);
      }
      setSelection({ kind: "region", id: null, regionId, focusField: "max_cost_usd" });
    },
    [startRunEditing, setActiveTab, setSelection],
  );
}
