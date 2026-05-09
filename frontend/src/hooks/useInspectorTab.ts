import { useState, useCallback } from "react";

export type InspectorTab = "run" | "edit";

const ACTIVE_RUN_STATUSES = new Set(["running", "awaiting_user", "halted"]);

export function useInspectorTab(
  pipelineKey: string | null,
  runStatus: string | null,
  isEditingRun: boolean,
) {
  const [state, setState] = useState<{
    key: string | null;
    tab: InspectorTab | null;
  }>({ key: null, tab: null });

  const tabOverride = state.key === pipelineKey ? state.tab : null;

  const setActiveTab = useCallback(
    (tab: InspectorTab | null) => {
      setState({ key: pipelineKey, tab });
    },
    [pipelineKey],
  );

  const isActiveRun =
    isEditingRun && runStatus != null && ACTIVE_RUN_STATUSES.has(runStatus);
  const contextDefault: InspectorTab = isActiveRun ? "run" : "edit";
  const activeTab = tabOverride ?? contextDefault;

  return { activeTab, setActiveTab };
}
