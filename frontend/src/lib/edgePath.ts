// Rendering helpers for orthogonal edges (#154). Pure functions shared by the
// custom xyflow edge component — kept out of the component so they can be unit
// tested without a DOM/flow harness.

import type { Point } from "./orthogonalRouter";

/**
 * Converts a polyline into an SVG path `d` string of straight line segments
 * (`M x,y L x,y ...`). Returns "" for a degenerate path (< 2 points).
 */
export function pathToSvg(points: Point[]): string {
  if (points.length < 2) return "";
  const [first, ...rest] = points;
  const head = `M ${first.x},${first.y}`;
  const tail = rest.map((p) => `L ${p.x},${p.y}`).join(" ");
  return `${head} ${tail}`;
}

export type SegmentOrientation = "horizontal" | "vertical";

export interface SegmentHandle {
  /** Index of the segment this handle controls (between points[i] and [i+1]). */
  segmentIndex: number;
  /** Handle anchor — the midpoint of the segment, in flow coordinates. */
  x: number;
  y: number;
  /**
   * The segment's orientation. A horizontal segment is dragged vertically and a
   * vertical segment horizontally — handles move perpendicular to their segment
   * only (design screen 14).
   */
  orientation: SegmentOrientation;
}

/**
 * Computes one segment handle per polyline segment, anchored at the segment's
 * midpoint, tagged with the segment's orientation. Zero-length segments are
 * skipped (they offer no perpendicular drag axis).
 */
export function segmentHandles(points: Point[]): SegmentHandle[] {
  const handles: SegmentHandle[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    const a = points[i];
    const b = points[i + 1];
    const horizontal = Math.abs(a.y - b.y) < 1e-6;
    const vertical = Math.abs(a.x - b.x) < 1e-6;
    if (horizontal && Math.abs(a.x - b.x) < 1e-6) continue; // zero-length
    if (vertical && Math.abs(a.y - b.y) < 1e-6) continue; // zero-length
    handles.push({
      segmentIndex: i,
      x: (a.x + b.x) / 2,
      y: (a.y + b.y) / 2,
      orientation: horizontal ? "horizontal" : "vertical",
    });
  }
  return handles;
}

/**
 * Drags segment `segmentIndex` along its perpendicular axis to `coord`,
 * returning the new orthogonal polyline. A horizontal segment moves in `y`, a
 * vertical segment in `x`. Graph endpoints (the first and last point) stay
 * anchored to their node: an endpoint can't move, so a new bend is inserted
 * beside it that carries `coord` on the moving axis while the endpoint keeps
 * its position, leaving the path right-angled. When the dragged segment touches
 * an endpoint at both ends (e.g. a straight two-point edge), a bend is inserted
 * on each side. This is what turns the first manual drag into pinned waypoints.
 */
export function dragSegment(
  points: Point[],
  segmentIndex: number,
  coord: number,
): Point[] {
  if (segmentIndex < 0 || segmentIndex >= points.length - 1) return points;
  const result = points.map((p) => ({ ...p }));
  const aIdx = segmentIndex;
  const bIdx = segmentIndex + 1;
  const horizontal = Math.abs(result[aIdx].y - result[bIdx].y) < 1e-6;
  // The axis the drag moves the segment along: a horizontal segment slides in
  // y, a vertical one in x. The other axis is fixed for each segment end.
  const axis: "x" | "y" = horizontal ? "y" : "x";
  const fixed: "x" | "y" = horizontal ? "x" : "y";

  const lastIdx = result.length - 1;
  const isEndpoint = (i: number) => i === 0 || i === lastIdx;

  // Move the `b` end first so inserting a bend before it doesn't shift `a`'s
  // index. For an anchored endpoint, insert a bend carrying the endpoint's
  // fixed coordinate at the new `coord`; for an interior point, slide it.
  const bEnd = result[bIdx];
  if (isEndpoint(bIdx)) {
    result.splice(bIdx, 0, { ...bEnd, [axis]: coord, [fixed]: bEnd[fixed] });
  } else {
    bEnd[axis] = coord;
  }

  const aEnd = result[aIdx];
  if (isEndpoint(aIdx)) {
    result.splice(aIdx + 1, 0, { ...aEnd, [axis]: coord, [fixed]: aEnd[fixed] });
  } else {
    aEnd[axis] = coord;
  }

  return result;
}

/**
 * Re-anchors persisted manual waypoints against the current endpoints so that
 * `[source, ...waypoints, target]` stays orthogonal after a connected node
 * moves (#165). Waypoints are stored as absolute coordinates pinned to the
 * endpoint positions at drag time; when an endpoint follows its node, the
 * segment between it and the adjacent (stale) waypoint would otherwise go
 * diagonal.
 *
 * The fix only touches the two endpoint-adjacent waypoints. The first segment's
 * orientation is read from the interior chain (`w0 -> w1` alternates with
 * `source -> w0`), so the source-adjacent waypoint follows the source on the
 * shared axis; symmetric at the target end. Interior waypoints are left
 * untouched, preserving the user's shaping.
 */
export function reanchorWaypoints(
  source: Point,
  target: Point,
  waypoints: Point[],
): Point[] {
  const out = waypoints.map((p) => ({ ...p }));
  if (out.length === 0) return out;

  // A single waypoint is an L-bend touching both endpoints, so it has no
  // interior chain to read orientation from. The two orthogonal elbows are
  // {source.x, target.y} (vertical-then-horizontal) and {target.x, source.y}
  // (horizontal-then-vertical); keep the user's shape by snapping to whichever
  // the stale waypoint is closer to.
  if (out.length === 1) {
    const w = out[0];
    const vh = { x: source.x, y: target.y };
    const hv = { x: target.x, y: source.y };
    const dVh = Math.abs(w.x - vh.x) + Math.abs(w.y - vh.y);
    const dHv = Math.abs(w.x - hv.x) + Math.abs(w.y - hv.y);
    out[0] = dVh <= dHv ? vh : hv;
    return out;
  }

  const first = out[0];
  const second = out[1];
  // `w0 -> w1` horizontal ⇒ `source -> w0` is vertical (share x); else share y.
  const firstSegHorizontal = Math.abs(first.y - second.y) < 1e-6;
  if (firstSegHorizontal) {
    first.x = source.x;
  } else {
    first.y = source.y;
  }

  const last = out[out.length - 1];
  const penultimate = out[out.length - 2];
  // `wn-1 -> wn` horizontal ⇒ `wn -> target` is vertical (share x); else share y.
  const lastSegHorizontal = Math.abs(last.y - penultimate.y) < 1e-6;
  if (lastSegHorizontal) {
    last.x = target.x;
  } else {
    last.y = target.y;
  }

  return out;
}
