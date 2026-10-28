import type { ReactFlowInstance } from "@xyflow/react";
import { useEffect, useMemo, useRef } from "react";
import { useUiScale } from "../../lib/uiScale";
import { CANVAS_FIT_PADDING, type CanvasFitViewOptions } from "./usePaneToggleReframe";

/**
 * The UI scale, on the canvas. The canvas leaves the app's CSS zoom
 * (`pdo-unzoom`: React Flow reads pointers and rects in screen pixels, which a
 * CSS zoom would skew) and carries the scale in its viewport zoom instead: the
 * zoom bounds and the fit are multiplied by it, and a new scale re-frames the
 * canvas at its size.
 */
export function useCanvasScale(reactFlow: ReactFlowInstance): { zoom: number; fitViewOptions: CanvasFitViewOptions } {
  const zoom = useUiScale() / 100;
  const fitViewOptions = useMemo(() => ({ maxZoom: zoom, padding: CANVAS_FIT_PADDING }), [zoom]);
  const framedAt = useRef(zoom);
  useEffect(() => {
    if (framedAt.current === zoom) return;
    framedAt.current = zoom;
    void reactFlow.fitView({ maxZoom: zoom, padding: CANVAS_FIT_PADDING });
  }, [zoom, reactFlow]);
  return { zoom, fitViewOptions };
}
