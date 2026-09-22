// What a freshly drawn edge REMEMBERS of the gesture that drew it (#844).
//
// Kept out of `EditCanvas.onConnectEnd` so the rule — which layout fields a drop
// writes, and which it deliberately leaves absent — can be read and tested
// without an xyflow drag. The canvas measures (press point, drop point, node
// rects); this decides.

import type { EdgeAnchor, EdgeDef, PortSide } from "../types";
import type { Point } from "./orthogonalRouter";
import { enforcePerpendicularEnds, landingLeg, storableWaypoints } from "./anchorSide";
import { WIRING_GRID_STEP } from "./wiringGrid";

export interface DrawnEdgeInput {
  /** The polyline the preview drew, in flow coordinates. */
  traced: Point[];
  /** Where on the source rim the gesture started. Absent when the drag did not
   *  start on a rim strip (a structural handle, or a synthetic connection). */
  sourceAnchor: EdgeAnchor | null;
  /** Where on the target the arrow landed. Absent for a declared-port target
   *  (End's `result`, a merge's input), which keeps its fixed side. */
  targetAnchor: EdgeAnchor | null;
  /** The side the arrow arrives on. */
  targetSide: PortSide;
  /** Whether this target anchors by drop position at all (emergent bodies only). */
  anchorsByDrop: boolean;
}

/**
 * The layout patch to apply to the edge `onConnect` has just appended.
 *
 * Deliberate absences, all of which make an untouched pipeline round-trip byte
 * for byte:
 *
 *   - `target_side` is written only when the drop chose something other than
 *     `left`, which is the legacy default (#168);
 *   - the route is pinned only when there is something BETWEEN the two
 *     perpendicular legs — a straight two-leg wire is exactly what the auto
 *     router draws, so pinning it would be noise that later fights node moves.
 */
export function drawnEdgeLayout(input: DrawnEdgeInput): Partial<EdgeDef> {
  const { traced, sourceAnchor, targetAnchor, targetSide, anchorsByDrop } = input;

  // Enforced BEFORE persisting, not only while previewing: what is saved has to
  // end with the perpendicular leg into `target_anchor` and begin with one out of
  // `source_anchor`, or the reloaded edge would differ from the one drawn. Only
  // the points between the two legs are stored — the legs are re-derived from the
  // anchors on every render.
  const enforced =
    traced.length >= 2 && sourceAnchor
      ? enforcePerpendicularEnds(
          traced,
          sourceAnchor.side,
          targetSide,
          landingLeg(WIRING_GRID_STEP),
        )
      : traced;
  const interior = storableWaypoints(enforced).map((p) => ({
    x: Math.round(p.x),
    y: Math.round(p.y),
  }));

  const updates: Partial<EdgeDef> = {};
  if (anchorsByDrop && targetSide !== "left") updates.target_side = targetSide;
  if (sourceAnchor) updates.source_anchor = sourceAnchor;
  if (targetAnchor) updates.target_anchor = targetAnchor;
  if (interior.length > 0) {
    updates.mode = "manual";
    updates.waypoints = interior;
  }
  return updates;
}
