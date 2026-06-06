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
 * anchored to their node: when the dragged segment touches an endpoint, a new
 * bend point is inserted so the endpoint keeps its position and the path stays
 * right-angled. This is what turns the first manual drag into pinned waypoints.
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
  const a = result[aIdx];
  const b = result[bIdx];
  const horizontal = Math.abs(a.y - b.y) < 1e-6;

  const lastIdx = result.length - 1;
  const aIsEndpoint = aIdx === 0 || aIdx === lastIdx;
  const bIsEndpoint = bIdx === 0 || bIdx === lastIdx;

  if (horizontal) {
    // Move the segment in y. Anchored endpoints can't move, so insert a bend.
    if (aIsEndpoint) {
      result.splice(bIdx, 0, { x: a.x, y: coord });
      // b shifted right by one; move the (new) far end of the segment too.
      result[bIdx + 1].y = coord;
    } else {
      a.y = coord;
      if (bIsEndpoint) {
        result.splice(bIdx, 0, { x: b.x, y: coord });
      } else {
        b.y = coord;
      }
    }
  } else {
    // Vertical segment: move in x.
    if (aIsEndpoint) {
      result.splice(bIdx, 0, { x: coord, y: a.y });
      result[bIdx + 1].x = coord;
    } else {
      a.x = coord;
      if (bIsEndpoint) {
        result.splice(bIdx, 0, { x: coord, y: b.y });
      } else {
        b.x = coord;
      }
    }
  }
  return result;
}
