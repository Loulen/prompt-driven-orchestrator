import { useCallback } from "react";
import type { Node } from "@xyflow/react";
import { useEditStore } from "./editStore";
import { IDENTITY_SHIFT, type CanvasShift } from "./canvasShift";

/** The canvas's `onNodeDragStop`: persists the final positions of the dragged
 *  pipeline nodes and notes. A position is drawn; the file gets the stored one
 *  (`shift`), read against the canvas without what was dragged — the shift is
 *  frozen during the drag and drawn again from what is written. */
export function useCanvasNodeDragStop(shift: CanvasShift = IDENTITY_SHIFT) {
  const updateNodeViews = useEditStore((s) => s.updateNodeViews);
  const moveNote = useEditStore((s) => s.moveNote);
  return useCallback(
    (_: unknown, _node: Node, nodes: Node[]) => {
      // xyflow hands us EVERY dragged node (the selected set, or just [node] for
      // a single drag) with final positions, in ONE call (#232). Partition by
      // type: pipeline nodes persist via `updateNodeViews`, notes via `moveNote`
      // (#307 trap #1). A note is NOT in `pipeline.nodes`, so `updateNodeViews`
      // would silently drop its move (unknown id → ignored) and it would snap
      // back on the next re-derivation. Decorative loop-region boxes are skipped
      // (draggable:false already, belt-and-suspenders against config drift).
      const movedNodes: { id: string; x: number; y: number }[] = [];
      const moving = new Set(nodes.map((n) => n.id));
      const stored = (y: number) => Math.round(shift.toStored(y, moving));
      for (const n of nodes) {
        if (n.type === "loopRegion") continue;
        if (n.type === "note") {
          moveNote(n.id, n.position.x, stored(n.position.y));
          continue;
        }
        movedNodes.push({ id: n.id, x: n.position.x, y: stored(n.position.y) });
      }
      if (movedNodes.length > 0) updateNodeViews(movedNodes);
    },
    [updateNodeViews, moveNote, shift],
  );
}
