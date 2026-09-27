/**
 * Bring a card into frame when a guided tour asks for it (#825).
 */

import type { ReactFlowInstance } from "@xyflow/react";
import { useEffect } from "react";
import { CARD_HEIGHT, CARD_WIDTH } from "../../lib/nodePlacement";
import { registerCanvasReveal } from "../../lib/canvasReveal";
// #723 — orchestrator pastilles on the run-view node card.

export function useCanvasReveal(reactFlowRef: React.RefObject<HTMLDivElement | null>, reactFlow: ReactFlowInstance) {
  // #825 (FP iteration 2): the canvas's answer to `scrollIntoView`. A card is
  // inside a transformed viewport, so scrolling the page never reaches it — a
  // guided tour pointing at a node the reader has panned away from would light a
  // rectangle off screen and wait forever. Panning only ever happens when the
  // card is NOT already in frame: whoever asks is describing a wish, and a canvas
  // that re-centred on every ask would be yanking itself around under the mouse.
  useEffect(
    () =>
      registerCanvasReveal((nodeId) => {
        const wrapper = reactFlowRef.current;
        if (!wrapper) return;
        const rect = wrapper.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) return;
        const node = reactFlow.getInternalNode(nodeId);
        if (!node) return;
        const { x, y } = node.internals.positionAbsolute;
        const w = node.measured.width ?? CARD_WIDTH;
        const h = node.measured.height ?? CARD_HEIGHT;
        const topLeft = reactFlow.screenToFlowPosition({ x: rect.left, y: rect.top });
        const bottomRight = reactFlow.screenToFlowPosition({ x: rect.right, y: rect.bottom });
        const inFrame =
          x >= topLeft.x && y >= topLeft.y && x + w <= bottomRight.x && y + h <= bottomRight.y;
        if (inFrame) return;
        reactFlow.setCenter(x + w / 2, y + h / 2, { zoom: reactFlow.getZoom(), duration: 250 });
      }),
    [reactFlow, reactFlowRef],
  );
}
