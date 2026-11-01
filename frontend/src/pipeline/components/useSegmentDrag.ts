import { useCallback, useRef } from "react";
import { useReactFlow } from "@xyflow/react";
import { dragSegmentKeepingRun, storableWaypoints } from "../../lib/polylineCleanup";
import { enforcePerpendicularEnds } from "../../lib/anchorLanding";
import { mergeAlignedSegments, snapStepAtZoom } from "../../lib/wiringGrid";
import { useWiringStore } from "../../stores/wiringStore";
import { useEditStore } from "../store/editStore";
import { storedEdgePatch, useCanvasShift } from "../store/canvasShift";
import type { EdgeRoute } from "./useEdgeRoute";

/**
 * The drag of one segment handle of a selected edge (#178, #844): only that
 * segment moves, snapped to the wiring grid (Shift frees it), the legs stay
 * perpendicular, and aligning two segments on release merges the waypoint
 * between them.
 */
export function useSegmentDrag(
  edgeIndex: number | undefined,
  route: EdgeRoute,
): (segmentIndex: number, orientation: "horizontal" | "vertical") => (e: React.PointerEvent) => void {
  const storeUpdateEdge = useEditStore((s) => s.updateEdge);
  // The segment is dragged where it is drawn; the file gets where it is stored.
  const shift = useCanvasShift();
  const updateEdge = useCallback(
    (index: number, updates: Parameters<typeof storeUpdateEdge>[1]) => storeUpdateEdge(index, storedEdgePatch(shift, updates)),
    [storeUpdateEdge, shift],
  );
  const { screenToFlowPosition, getZoom } = useReactFlow();
  const { points, srcSide, tgtSide, step, leg, tgtRect } = route;
  // Shift state during a segment drag, read off the live event (the pointer is
  // captured, so a keyboard listener would be the only other way in).
  const shiftRef = useRef(false);

  const onHandleDrag = useCallback(
    (segmentIndex: number, orientation: "horizontal" | "vertical") =>
      (e: React.PointerEvent) => {
        e.stopPropagation();
        // Primary button only. Without this, a right-click on a handle still
        // starts a drag and silently reshapes the route — the delete-waypoint
        // gesture is meant to be GONE (#844), not renamed.
        if (e.button !== 0) return;
        if (edgeIndex == null) return;
        // The session origin is read ONCE, at grab time: a segment drag belongs
        // to the gesture in progress (a Shift release inside it re-origins), and
        // freezing it here keeps the snap stable for the whole drag.
        const origin = useWiringStore.getState().origin;
        // Zoomed out, the segment snaps to a multiple of the step (CORE-88), so
        // a cell stays big enough to aim at and still lies on the step's lattice.
        const snapStep = snapStepAtZoom(step, getZoom()).step;
        let latest = points;
        let accepted = false;
        const move = (ev: PointerEvent) => {
          shiftRef.current = ev.shiftKey;
          const flow = screenToFlowPosition({ x: ev.clientX, y: ev.clientY });
          const coord = orientation === "horizontal" ? flow.y : flow.x;
          // Only THIS segment moves. `dragSegmentOnGrid` never rewrites the
          // neighbours' coordinates, so the rest of the route is byte-stable.
          const next = dragSegmentKeepingRun(
            points,
            segmentIndex,
            coord,
            { origin, step: snapStep, free: ev.shiftKey },
            (pts) => enforcePerpendicularEnds(pts, srcSide, tgtSide, leg, tgtRect ?? undefined),
            tgtRect ?? undefined,
          );
          // A position the route cannot keep the run at (across the target
          // card): hold the last one it could, instead of drawing the wire
          // jumping away from the pointer.
          if (!next.keepsRun && accepted) return;
          accepted = true;
          latest = next.moved;
          const enforced = next.enforced;
          updateEdge(edgeIndex, {
            mode: "manual",
            waypoints: storableWaypoints(enforced).map((p) => ({
              x: Math.round(p.x),
              y: Math.round(p.y),
            })),
          });
        };
        const up = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
          // Releasing Shift at the end of the drag re-origins the grid on the
          // point just placed (ADR-0072) — the same semantics as while wiring.
          if (shiftRef.current) {
            const tip = latest[Math.min(segmentIndex + 1, latest.length - 1)];
            if (tip) useWiringStore.getState().setOrigin({ x: tip.x, y: tip.y });
            shiftRef.current = false;
          }
          // Aligning two segments merges the waypoint between them (#844). That is
          // the ONLY way to delete a waypoint now.
          const merged = enforcePerpendicularEnds(
            mergeAlignedSegments(latest),
            srcSide,
            tgtSide,
            leg,
            tgtRect ?? undefined,
          );
          const interior = storableWaypoints(merged);
          updateEdge(edgeIndex, {
            mode: interior.length > 0 ? "manual" : "auto",
            waypoints:
              interior.length > 0
                ? interior.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) }))
                : null,
          });
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
      },
    [edgeIndex, points, screenToFlowPosition, getZoom, updateEdge, srcSide, tgtSide, step, leg, tgtRect],
  );
  return onHandleDrag;
}
