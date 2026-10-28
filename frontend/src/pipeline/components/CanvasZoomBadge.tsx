import { Panel, useStore } from "@xyflow/react";
import { useUiScale } from "../../lib/uiScale";

/** The canvas zoom, bottom-left (CORE-88): the wiring snaps to a multiple of the
 *  grid step once dezoomed, and this says how far out the canvas is. The canvas
 *  carries the UI scale in its viewport zoom: 100 % is the size cards are drawn
 *  at, and the badge itself is drawn at the scale (`pdo-rezoom`). */
export default function CanvasZoomBadge() {
  const zoom = useStore((s) => s.transform[2]);
  const scale = useUiScale() / 100;
  return (
    <Panel position="bottom-left">
      <span
        data-testid="canvas-zoom"
        data-zoom={zoom}
        title="Canvas zoom"
        className="pdo-rezoom block rounded border border-line bg-surface-1 px-1 font-mono text-2xs text-text-3"
      >
        {Math.round((zoom / scale) * 100)}%
      </span>
    </Panel>
  );
}
