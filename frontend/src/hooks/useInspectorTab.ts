import { useState, useCallback } from "react";

export type InspectorTab = "run" | "edit";

export function useInspectorTab(
  pipelineKey: string | null,
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

  // #271: any run tab — live OR terminal — defaults to the Run view so a
  // finished node lands on its Outputs. Non-run tabs (library drafts) → edit.
  // The user's manual choice (tabOverride) still wins and is sticky per tab.
  const contextDefault: InspectorTab = isEditingRun ? "run" : "edit";
  const activeTab = tabOverride ?? contextDefault;

  return { activeTab, setActiveTab };
}
