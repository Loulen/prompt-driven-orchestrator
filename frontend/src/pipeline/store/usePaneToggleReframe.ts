/**
 * Re-frame the canvas when App's right pane folds or unfolds (#949).
 */

import type { ReactFlowInstance } from "@xyflow/react";
import { useEffect, useLayoutEffect, useRef } from "react";
import { CARD_HEIGHT, CARD_WIDTH } from "../../lib/nodePlacement";
import { reframeOnPaneResize } from "../../lib/paneResizeViewport";
import { useEditStore } from "./editStore";

export function usePaneToggleReframe(
  reactFlowRef: React.RefObject<HTMLDivElement | null>,
  reactFlow: ReactFlowInstance,
  hasCanvas: boolean,
  rightPaneCollapsed: boolean,
) {
  // #949 (FP iteration 1): the right pane folding or unfolding resizes the
  // canvas from the right; without this, a graph fit-viewed on the full-width
  // canvas slides under the unfolded pane, clicked card included. The toggle
  // arms a one-shot flag (child layout effects run before App's, which resizes
  // the pane); the ResizeObserver, which fires before paint, consumes it on the
  // width change it causes. A drag of the handle or a window resize never arms
  // it, so they keep React Flow's own top-left anchoring. The flag disarms
  // itself if no resize follows, so a later unrelated resize cannot consume it.
  const lastCanvasWidthRef = useRef<number | null>(null);
  const paneToggledRef = useRef(false);
  const prevRightCollapsedRef = useRef(rightPaneCollapsed);
  useLayoutEffect(() => {
    if (prevRightCollapsedRef.current === rightPaneCollapsed) return;
    prevRightCollapsedRef.current = rightPaneCollapsed;
    paneToggledRef.current = true;
    const disarm = setTimeout(() => {
      paneToggledRef.current = false;
    }, 200);
    return () => clearTimeout(disarm);
  }, [rightPaneCollapsed]);
  useEffect(() => {
    const wrapper = reactFlowRef.current;
    if (!hasCanvas || !wrapper || typeof ResizeObserver === "undefined") return;
    lastCanvasWidthRef.current = null;
    const observer = new ResizeObserver(() => {
      const { width, height } = wrapper.getBoundingClientRect();
      const prevWidth = lastCanvasWidthRef.current;
      lastCanvasWidthRef.current = width;
      if (!paneToggledRef.current || prevWidth == null || prevWidth <= 0 || width <= 0) return;
      if (width === prevWidth) return;
      paneToggledRef.current = false;
      const selection = useEditStore.getState().selection;
      const node = selection.kind === "node" && selection.id ? reactFlow.getInternalNode(selection.id) : undefined;
      const selected = node
        ? {
            ...node.internals.positionAbsolute,
            width: node.measured.width ?? CARD_WIDTH,
            height: node.measured.height ?? CARD_HEIGHT,
          }
        : null;
      const nodes = reactFlow.getNodes();
      const graph = nodes.length > 0 ? reactFlow.getNodesBounds(nodes) : null;
      reactFlow.setViewport(reframeOnPaneResize(reactFlow.getViewport(), prevWidth, width, height, selected, graph));
    });
    observer.observe(wrapper);
    return () => observer.disconnect();
  }, [hasCanvas, reactFlow, reactFlowRef]);
}
