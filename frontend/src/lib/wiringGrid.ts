// Wiring grid — pure geometry for the progressive, grid-snapped edge tracing of
// #844 (ADR-0072). Free of React and of xyflow so the shapes can be reasoned
// about — and unit-tested — on their own, next to `edgePath.ts`.
//
// The vocabulary is CONTEXT.md § « Routage — grille de câblage » : the wiring
// grid is the lattice a trace snaps to, distinct from the decorative background
// grid (20px, xyflow `Background`) which is pure chrome.

import type { Point } from "./orthogonalRouter";

/**
 * The wiring grid step, in flow px. A **product constant, never a setting**
 * (ADR-0072): `waypoints` travel inside the pipeline file, so two instances on
 * different steps would render the same YAML with routes that no longer line up,
 * and every drag would re-snap onto a lattice foreign to the drawn trace.
 *
 * 40px was chosen on the #840 prototype after trying 20px: a coarse lattice is
 * what makes two wires drawn a moment apart agree. A finer step snapped to
 * *something* on every pixel of travel, which is barely a grid at all.
 */
export const WIRING_GRID_STEP = 40;

export type Axis = "horizontal" | "vertical";

/** Snaps `p` onto the grid of pitch `step` whose lattice passes through `origin`. */
export function snapToGrid(p: Point, origin: Point, step: number): Point {
  return {
    x: origin.x + Math.round((p.x - origin.x) / step) * step,
    y: origin.y + Math.round((p.y - origin.y) / step) * step,
  };
}

const EPS = 1e-6;

const sameX = (a: Point, b: Point) => Math.abs(a.x - b.x) < EPS;
const sameY = (a: Point, b: Point) => Math.abs(a.y - b.y) < EPS;

/** True when the two points share an axis, i.e. the segment between them is orthogonal. */
export function isAxisAligned(a: Point, b: Point): boolean {
  return sameX(a, b) || sameY(a, b);
}

/** Orientation of the segment a→b. A zero-length segment reports `null`. */
export function segmentAxis(a: Point, b: Point): Axis | null {
  if (sameX(a, b) && sameY(a, b)) return null;
  return sameY(a, b) ? "horizontal" : "vertical";
}

/**
 * Drops points that sit in the middle of a straight run, so a trace that walked
 * three cells in one direction is three points, not four. Also drops exact
 * duplicates. This is what makes the incremental tracing self-correcting: a user
 * who overshoots a cell and comes back gets the extra waypoint absorbed.
 */
export function mergeCollinear(points: Point[]): Point[] {
  const out: Point[] = [];
  for (const p of points) {
    const last = out[out.length - 1];
    if (last && sameX(last, p) && sameY(last, p)) continue; // duplicate
    out.push({ ...p });
    while (out.length >= 3) {
      const [a, b, c] = out.slice(-3);
      const collinear = (sameY(a, b) && sameY(b, c)) || (sameX(a, b) && sameX(b, c));
      if (!collinear) break;
      out.splice(out.length - 2, 1); // the middle point carries no bend
    }
  }
  return out;
}

/**
 * The elbow of the L from `from` to `to` that leaves along `incoming` first —
 * "straight, then turn". Keeping the incoming direction is what makes the trace
 * feel like it is being *drawn* rather than recomputed: the pencil carries on the
 * way it was going and only then corners.
 */
export function elbow(from: Point, to: Point, incoming: Axis): Point {
  return incoming === "horizontal" ? { x: to.x, y: from.y } : { x: from.x, y: to.y };
}

/** A trace being drawn: committed bends plus the moving tip. */
export interface Trace {
  /** Committed points, `points[0]` being the departure point on the node rim. */
  points: Point[];
  /** Direction the last committed segment travels in — where the pencil points. */
  heading: Axis;
}

export function startTrace(origin: Point, heading: Axis): Trace {
  return { points: [{ ...origin }], heading };
}

/**
 * Advances the trace to `to` (already snapped, or free under Shift), adding at
 * most one bend. A move that stays on the trace's current axis just slides the
 * tip; a move off-axis commits an elbow. `mergeCollinear` then absorbs any bend
 * the move has straightened out, so retracing your steps un-draws them.
 */
export function advanceTrace(trace: Trace, to: Point): Trace {
  const points = trace.points;
  const anchor = points.length >= 2 ? points[points.length - 2] : points[0];
  const tip = points[points.length - 1];
  if (sameX(tip, to) && sameY(tip, to)) return trace;

  let next: Point[];
  if (points.length >= 2 && isAxisAligned(anchor, to)) {
    // The tip can reach `to` without a new bend: slide it.
    next = [...points.slice(0, -1), { ...to }];
  } else if (points.length === 1) {
    const e = elbow(tip, to, trace.heading);
    next = mergeCollinear([tip, e, to]);
  } else {
    const e = elbow(tip, to, segmentAxis(anchor, tip) ?? trace.heading);
    next = [...points, e, { ...to }];
  }

  const merged = mergeCollinear(next);
  const a = merged[merged.length - 2];
  const b = merged[merged.length - 1];
  return { points: merged, heading: (a && segmentAxis(a, b)) ?? trace.heading };
}

/**
 * Releasing Shift **re-origins the grid on the current point** (ADR-0072): the
 * freehand run is kept verbatim and the lattice is redefined to pass through
 * where the pencil now is, so the next cells align with what was just drawn
 * instead of with where the gesture happened to start.
 */
export function reOrigin(point: Point): Point {
  return { x: point.x, y: point.y };
}

/**
 * The free (Shift-held) continuation: still an orthogonal L — never a staircase —
 * leaving along the current heading, but ignoring the grid.
 */
export function freeContinuation(trace: Trace, cursor: Point): Point[] {
  const tip = trace.points[trace.points.length - 1];
  if (isAxisAligned(tip, cursor)) return mergeCollinear([...trace.points, cursor]);
  return mergeCollinear([...trace.points, elbow(tip, cursor, trace.heading), cursor]);
}

/**
 * Drags interior segment `segmentIndex` of `points` onto `coord` along its
 * perpendicular axis, snapping to the grid unless `free`. Unlike `dragSegment`
 * (`edgePath.ts`), this NEVER touches the neighbouring waypoints' coordinates
 * (#844 AC « les waypoints voisins conservent leurs coordonnées exactes ») and
 * lets the drag align the segment with a neighbour so the waypoint between them
 * can be merged away on release.
 */
export function dragSegmentOnGrid(
  points: Point[],
  segmentIndex: number,
  coord: number,
  opts: { origin: Point; step: number; free: boolean; magnet?: boolean },
): Point[] {
  if (segmentIndex < 0 || segmentIndex >= points.length - 1) return points;
  const result = points.map((p) => ({ ...p }));
  const a = result[segmentIndex];
  const b = result[segmentIndex + 1];
  const axis: "x" | "y" = sameY(a, b) ? "y" : "x";
  const snapped = opts.free ? coord : snapAxis(coord, axis, opts.origin, opts.step);

  // Magnet on the neighbours. Merging two segments means landing this one
  // EXACTLY on the coordinate of the segment two along — and a neighbour pinned
  // to a node sits wherever the card sits, which is almost never on a grid line.
  // Snapping alone therefore makes the merge gesture unreachable: the route stops
  // one grid step short, every time, with no way to close the gap. So a
  // neighbour's own coordinate is a candidate too, and it wins inside half a cell
  // — the grid guides, the neighbour snaps shut.
  const neighbours = [result[segmentIndex - 1], result[segmentIndex + 2]]
    .filter((p): p is Point => p != null)
    .map((p) => p[axis]);
  // `magnet: false` is the grid alone — for when the neighbour's coordinate is
  // one the route cannot keep (see `dragSegmentKeepingRun`).
  const target = opts.free || opts.magnet === false
    ? snapped
    : nearestCandidate(coord, [snapped, ...neighbours], opts.step / 2);

  const lastIdx = result.length - 1;
  // An endpoint is pinned to its node, so a bend is inserted beside it to carry
  // the drag while the endpoint stays put. Insert at `b` first so `a`'s index
  // survives.
  if (segmentIndex + 1 === lastIdx) {
    result.splice(segmentIndex + 1, 0, { ...b, [axis]: target });
  } else {
    b[axis] = target;
  }
  if (segmentIndex === 0) {
    result.splice(1, 0, { ...a, [axis]: target });
  } else {
    a[axis] = target;
  }
  return result;
}

function snapAxis(coord: number, axis: "x" | "y", origin: Point, step: number): number {
  const p = axis === "y" ? { x: 0, y: coord } : { x: coord, y: 0 };
  return snapToGrid(p, origin, step)[axis];
}

/** The candidate nearest `coord`, preferring an exact neighbour match inside
 *  `tolerance`; falls back to the first candidate (the grid position). */
function nearestCandidate(coord: number, candidates: number[], tolerance: number): number {
  let best = candidates[0];
  let bestDist = Infinity;
  for (const c of candidates.slice(1)) {
    const dist = Math.abs(c - coord);
    if (dist <= tolerance && dist < bestDist) {
      best = c;
      bestDist = dist;
    }
  }
  return best;
}

/**
 * Called on drag release: two segments that ended up collinear become one and the
 * waypoint between them disappears (#844 — « aligner deux segments fusionne le
 * waypoint intermédiaire à la fin du drag »). That is the only way to delete a
 * waypoint; there is no right-click gesture on a handle any more.
 */
export function mergeAlignedSegments(points: Point[]): Point[] {
  return mergeCollinear(points);
}

/**
 * Snaps an already-orthogonal polyline onto the wiring grid while KEEPING it
 * orthogonal — what « Re-route automatically produit un tracé auto aligné sur la
 * grille » needs (#844). Snapping each point independently is the trap: it nudges
 * the two ends of a segment onto different grid lines and the route comes back as
 * a staircase of near-diagonals.
 *
 * So each interior point is snapped, and then only the coordinate its own segment
 * *shares* with its predecessor is restored — the free coordinate keeps the grid
 * value, the shared one keeps the path square. Endpoints never move: they belong
 * to their nodes.
 */
export function snapPolyline(points: Point[], origin: Point, step: number): Point[] {
  if (points.length < 3) return points;
  const axes = points.slice(0, -1).map((p, i) => segmentAxis(p, points[i + 1]));
  const out = points.map((p, i) =>
    i === 0 || i === points.length - 1 ? { ...p } : snapToGrid(p, origin, step),
  );
  for (let i = 1; i <= out.length - 2; i++) {
    if (axes[i - 1] === "horizontal") out[i].y = out[i - 1].y;
    else if (axes[i - 1] === "vertical") out[i].x = out[i - 1].x;
  }
  // The final segment has to land on the (immovable) target, so the last interior
  // point gives up the coordinate that segment shares with it.
  const last = out.length - 1;
  if (axes[last - 1] === "horizontal") out[last - 1].y = out[last].y;
  else if (axes[last - 1] === "vertical") out[last - 1].x = out[last].x;
  return mergeCollinear(out);
}

/**
 * The sub-pixel nudge, in flow px, that puts a grid line's left/top edge on a
 * whole SCREEN pixel.
 *
 * The viewport translate is routinely fractional (xyflow centres the fit view),
 * and a one-pixel line starting at x.5 is rasterised as two half-intensity
 * columns: at zoom 1 exactly, where users sit, the `lines` grid came out visibly
 * fainter than at 0.95 (#844 FP iter-2). Nudging by under half a screen pixel is
 * invisible against the 1.5px wire and keeps the line crisp. Exact for every line
 * whenever `zoom × step` is whole (zoom 1, 0.5, 2…); elsewhere the lines fall on
 * varying fractions anyway and this only aligns the one through the origin.
 */
export function crispOffset(latticeFlow: number, translate: number, zoom: number): number {
  const screen = translate + zoom * latticeFlow;
  return (Math.round(screen) - screen) / zoom;
}
