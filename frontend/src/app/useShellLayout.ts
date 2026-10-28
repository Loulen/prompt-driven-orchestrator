import { useState } from "react";
import { useResizableLayout } from "../hooks/useResizableLayout";
import { useTerminalOverlayOpen } from "../session/store/terminalOverlayStore";
import { defaultPaneSizes, PANE_BOUNDS } from "../lib/paneSizes";
import { cssViewport } from "../shared/ui/zoom";
import { useCollapsibleRightPane } from "./useCollapsibleRightPane";

const PANEL_IDS = ["left", "center", "right"];
/** #949: the right pane collapses on its own; the canvas takes its width meanwhile. */
const RIGHT_COLLAPSIBLE = { panelId: "right", absorbInto: "center" };

/**
 * The three panes of the Workbench: their persisted layout, the bounds of the
 * two side panels in pixels, the inspector that collapses when empty (#949),
 * and whether an enlarged node terminal freezes the resizing (#44). The left
 * panel and the inspector open at a width in pixels, whatever the window (read
 * once, at the UI scale).
 */
export function useShellLayout(rightPaneCollapsed: boolean) {
  const [defaultSizes] = useState(() => defaultPaneSizes(cssViewport().width));
  const layout = useResizableLayout("run", PANEL_IDS, defaultSizes, RIGHT_COLLAPSIBLE);
  const rightPane = useCollapsibleRightPane(rightPaneCollapsed, layout.persistedSize, defaultSizes.right);
  const frozen = useTerminalOverlayOpen();
  const bounds = (side: "left" | "right") => ({
    defaultSize: layout.defaultLayout[side],
    minSize: `${PANE_BOUNDS[side].min}px`,
    maxSize: `${PANE_BOUNDS[side].max}px`,
  });
  return { layout, rightPane, frozen, left: bounds("left"), right: bounds("right") };
}
