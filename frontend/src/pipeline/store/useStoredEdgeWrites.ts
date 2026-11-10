import { useCallback, useRef } from "react";
import { useEditStore } from "./editStore";
import { IDENTITY_SHIFT, storedEdgePatch, type CanvasShift } from "./canvasShift";

export { CanvasShiftContext } from "./canvasShift";

/**
 * The drawn pipeline against the stored one (`model/grownCanvas.ts`), for the
 * gestures of the canvas that write a position — a drawn wire, a new card —
 * and run before the canvas has computed its shift for this render
 * (`useCanvasCompaction` keeps `shiftRef` on it); `updateEdge` writes an
 * edge's waypoints and labels where the file stores them.
 */
export function useStoredEdgeWrites() {
  const storeUpdateEdge = useEditStore((s) => s.updateEdge);
  const shiftRef = useRef<CanvasShift>(IDENTITY_SHIFT);
  const updateEdge = useCallback<typeof storeUpdateEdge>(
    (index, updates, opts) => storeUpdateEdge(index, storedEdgePatch(shiftRef.current, updates), opts),
    [storeUpdateEdge],
  );
  return { shiftRef, updateEdge };
}
