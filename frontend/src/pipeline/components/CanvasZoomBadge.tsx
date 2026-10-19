import { Panel, useStore } from "@xyflow/react";

/** The canvas zoom, bottom-left (CORE-88): the wiring snaps to a multiple of the
 *  grid step once dezoomed, and this says how far out the canvas is. */
export default function CanvasZoomBadge() {
  const zoom = useStore((s) => s.transform[2]);
  return (
    <Panel position="bottom-left">
      <span
        data-testid="canvas-zoom"
        data-zoom={zoom}
        className="rounded border border-line bg-bg-2 px-2 py-1 font-mono text-fg-3"
        style={{ fontSize: "10px" }}
      >
        zoom {Math.round(zoom * 100)} %
      </span>
    </Panel>
  );
}
