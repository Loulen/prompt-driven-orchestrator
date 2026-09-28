import { useCallback, useState } from "react";
import type { TabId } from "../pipeline/components/PipelineInfoPanel";
import { useEditStore } from "../pipeline/store/editStore";
import type { OpenPipeline } from "../pipeline/store/editState";

/** The pipeline info panel: open or not, and the tab (and YAML line) it opens on. */
export function useInfoPanel(saveErrorTab: OpenPipeline | undefined) {
  const clearSaveError = useEditStore((s) => s.clearSaveError);
  const [infoPanelOpen, setInfoPanelOpen] = useState(false);
  const [infoPanelInitialTab, setInfoPanelInitialTab] = useState<TabId | undefined>(undefined);
  const [infoPanelScrollToLine, setInfoPanelScrollToLine] = useState<number | undefined>(undefined);

  const handleToggleInfo = useCallback(() => {
    setInfoPanelOpen((prev) => {
      if (!prev) {
        setInfoPanelInitialTab(undefined);
        setInfoPanelScrollToLine(undefined);
      }
      return !prev;
    });
  }, []);

  const handleCloseInfo = useCallback(() => {
    setInfoPanelOpen(false);
  }, []);

  // #302 / ADR-0048: the toolbar Bot glyph opens the info panel focused on the
  // Assistant tab (the library authoring copilot). Same shape as `handleViewYaml`:
  // set the initial tab, then open. The panel is keyed on `infoPanelInitialTab`,
  // so this remounts it at the Assistant tab.
  const handleOpenAssistant = useCallback(() => {
    setInfoPanelInitialTab("assistant");
    setInfoPanelScrollToLine(undefined);
    setInfoPanelOpen(true);
  }, []);

  const handleViewYaml = useCallback(() => {
    if (!saveErrorTab) return;
    setInfoPanelInitialTab("yaml");
    setInfoPanelScrollToLine(saveErrorTab.saveError?.line);
    setInfoPanelOpen(true);
    clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);

  return {
    infoPanelOpen,
    setInfoPanelOpen,
    infoPanelInitialTab,
    infoPanelScrollToLine,
    handleToggleInfo,
    handleCloseInfo,
    handleOpenAssistant,
    handleViewYaml,
  };
}
