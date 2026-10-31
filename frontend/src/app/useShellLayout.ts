import { useCallback, useState } from "react";
import { useResizableLayout, type Layout } from "../hooks/useResizableLayout";
import { useTerminalOverlayOpen } from "../session/store/terminalOverlayStore";
import { CANVAS_MIN_PCT, defaultPaneSizes, PANE_BOUNDS, restorePaneSizes } from "../lib/paneSizes";
import { cssViewport } from "../shared/ui/zoom";
import { useCollapsibleRightPane } from "./useCollapsibleRightPane";

const PANEL_IDS = ["left", "center", "right"];
/** #949: the right pane collapses on its own; the canvas takes its width meanwhile. */
const RIGHT_COLLAPSIBLE = { panelId: "right", absorbInto: "center" };
/** The width, in CSS px, of the window the stored layout was saved in. */
const WIDTH_KEY = "pdo.layout.run.width";

function savedWidth(): number | null {
  try {
    const width = Number(localStorage.getItem(WIDTH_KEY));
    return width > 0 ? width : null;
  } catch {
    return null;
  }
}

/**
 * The three panes of the Workbench: their persisted layout, the bounds of the
 * two side panels in pixels, the inspector that collapses when empty (#949),
 * and whether an enlarged node terminal freezes the resizing (#44). The left
 * panel and the inspector open at a width in pixels, whatever the window, and
 * keep their pixels when the window or the UI scale changes (read at the UI
 * scale): the layout is saved with the width of its window.
 */
export function useShellLayout(rightPaneCollapsed: boolean) {
  const [width] = useState(() => cssViewport().width);
  const [defaultSizes] = useState(() => defaultPaneSizes(width));
  const [restore] = useState(
    () => (stored: Layout) =>
      restorePaneSizes({ left: stored.left, center: stored.center, right: stored.right }, savedWidth(), width),
  );
  const stored = useResizableLayout("run", PANEL_IDS, defaultSizes, RIGHT_COLLAPSIBLE, restore);
  const save = stored.onLayoutChanged;
  const onLayoutChanged = useCallback(
    (next: Layout) => {
      save(next);
      try {
        localStorage.setItem(WIDTH_KEY, String(Math.round(cssViewport().width)));
      } catch {
        // A disabled store: the layout lasts the session.
      }
    },
    [save],
  );
  const layout = { ...stored, onLayoutChanged };
  const rightPane = useCollapsibleRightPane(rightPaneCollapsed, layout.persistedSize, defaultSizes.right);
  const frozen = useTerminalOverlayOpen();
  const bounds = (side: "left" | "right") => ({
    defaultSize: layout.defaultLayout[side],
    minSize: `${PANE_BOUNDS[side].min}px`,
    maxSize: `${PANE_BOUNDS[side].max}px`,
    // A wider window or another UI scale widens the canvas, not the side panels.
    groupResizeBehavior: "preserve-pixel-size" as const,
  });
  // The canvas stops a drag at the floor a restore gives it: a reload shows what was dragged.
  const center = { defaultSize: layout.defaultLayout.center, minSize: `${CANVAS_MIN_PCT}%` };
  return { layout, rightPane, frozen, left: bounds("left"), center, right: bounds("right") };
}
