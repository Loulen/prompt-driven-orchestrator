import { useCallback, useState } from "react";
import {
  infoPanelButtons,
  toggleAssistantTab,
  toggleInfoTab,
  type InfoTabContext,
  type TabId,
} from "../lib/infoPanelReconcile";
import { useEditStore } from "../pipeline/store/editStore";
import type { OpenPipeline } from "../pipeline/store/editState";

/**
 * The pipeline info panel: open or not, the tab it shows (and the YAML line).
 * #938: the tab is App-owned (the panel is controlled), so the toolbar lights its
 * two buttons from the tab actually shown — never from the tab it opened on.
 */
export function useInfoPanel(saveErrorTab: OpenPipeline | undefined, ctx: InfoTabContext) {
  const clearSaveError = useEditStore((s) => s.clearSaveError);
  const [infoPanelOpen, setInfoPanelOpen] = useState(false);
  const [infoPanelTab, setInfoPanelTab] = useState<TabId>("info");
  const [infoPanelScrollToLine, setInfoPanelScrollToLine] = useState<number | undefined>(undefined);

  // #938 (story PDO-3): the "agent" glyph and `(i)` are mutually exclusive, lit
  // from the RESOLVED tab of the panel (`resolveInfoTab`, the rule the panel
  // renders with). Each closes the panel when it is the lit one, otherwise
  // opens — or switches — the panel on its own tab.
  const { hasRun, hasAssistant } = ctx;
  const { assistantActive, infoActive } = infoPanelButtons(
    { open: infoPanelOpen, tab: infoPanelTab },
    { hasRun, hasAssistant },
  );

  const applyToggle = useCallback(
    (toggle: typeof toggleInfoTab) => {
      const next = toggle({ open: infoPanelOpen, tab: infoPanelTab }, { hasRun, hasAssistant });
      setInfoPanelTab(next.tab);
      setInfoPanelScrollToLine(undefined);
      setInfoPanelOpen(next.open);
    },
    [infoPanelOpen, infoPanelTab, hasRun, hasAssistant],
  );

  const handleToggleInfo = useCallback(() => applyToggle(toggleInfoTab), [applyToggle]);

  const handleCloseInfo = useCallback(() => {
    setInfoPanelOpen(false);
  }, []);

  // #302 / ADR-0048: the toolbar Bot glyph toggles the info panel on the
  // Assistant tab (the library authoring copilot). No remount (#938): the
  // controlled tab just moves, so the Assistant keeps its conversation.
  const handleToggleAssistant = useCallback(() => applyToggle(toggleAssistantTab), [applyToggle]);

  const handleViewYaml = useCallback(() => {
    if (!saveErrorTab) return;
    setInfoPanelTab("yaml");
    setInfoPanelScrollToLine(saveErrorTab.saveError?.line);
    setInfoPanelOpen(true);
    clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);

  return {
    infoPanelOpen,
    setInfoPanelOpen,
    infoPanelTab,
    setInfoPanelTab,
    infoPanelScrollToLine,
    assistantActive,
    infoActive,
    handleToggleInfo,
    handleCloseInfo,
    handleToggleAssistant,
    handleViewYaml,
  };
}
