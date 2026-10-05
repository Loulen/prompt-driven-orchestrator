// Orthogonal edge router (issue #154, design screen 14 "Edge shaping").
//
// Pure, deterministic module: given two endpoints and a set of obstacle
// rectangles, produce a right-angle (axis-aligned) polyline that connects them
// and steers clear of the obstacles. This is the "auto" routing mode — no
// waypoints are persisted for auto edges; the path is recomputed on every
// render and re-routes for free when a node moves.
//
// The router is intentionally a deep module with a tiny surface: callers hand
// it geometry and get back a polyline, never the internal grid/search details.

import type { PortSide } from "../shared/generated/PortSide";

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RouteInput {
  source: Point;
  target: Point;
  obstacles: Rect[];
  /**
   * Clearance kept between the route and an obstacle (px). The routing grid is
   * built from obstacle edges expanded by this margin, so the path hugs nodes
   * at a readable distance rather than grazing their borders.
   */
  margin?: number;
  /**
   * The target card side the arrow anchors on (#168 / #175). The bend-minimal
   * path arrives perpendicular to this side, approaching from outside the card,
   * instead of always horizontally from the left. Absent ⇒ `left` (legacy
   * left-to-right arrival). Only the obstacle-free fast path honours this; the
   * A* fallback (rare — only when a node blocks the straight route) arrives
   * best-effort.
   */
  targetSide?: PortSide;
  /**
   * The source card side the wire leaves from, and the source card itself. The
   * source is not an obstacle (the wire starts on its border), so a route that
   * heads back across it — an output on the right towards a target below-left,
   * the typical « no `side` » output above its End — ran straight through the
   * card's label. Given both, such a route leaves by a stub on `sourceSide` and
   * goes around the card instead. Absent ⇒ the legacy behaviour.
   */
  sourceSide?: PortSide;
  sourceRect?: Rect;
  /**
   * Length of that stub (px). A caller that snaps the route onto a lattice
   * passes one that still clears its own perpendicular leg once rounded, or the
   * snapped bend lands short of the leg and draws a spur. Absent ⇒ `LEAD_IN`.
   */
  sourceStub?: number;
}

const DEFAULT_MARGIN = 16;

// How far outside the target card the non-left arrival route turns in from, so
// the final segment approaches the anchored side from outside the body rather
// than crossing it (#175).
const LEAD_IN = 22;

/**
 * Routes a right-angle polyline from `source` to `target`. The returned array
 * starts at `source`, ends at `target`, and contains only axis-aligned
 * segments (each consecutive pair shares an x or a y), avoiding the interior of
 * every obstacle rectangle.
 *
 * Fast path: the bend-minimal "HVH" step is used when it clears all obstacles.
 * Otherwise the router falls back to an A* search over a grid whose lines are
 * the endpoints and the margin-expanded obstacle edges — enough resolution to
 * thread between nodes without the cost of a fine pixel grid.
 */
export function routeOrthogonal(input: RouteInput): Point[] {
  const { source, target, obstacles } = input;
  const margin = input.margin ?? DEFAULT_MARGIN;
  const targetSide = input.targetSide ?? "left";

  const simple = rawPath(source, target, targetSide);
  const path = obstacles.some((o) => polylineHitsRect(simple, o))
    ? (routeViaGrid(source, target, obstacles, margin) ?? simple)
    : simple;

  const { sourceSide, sourceRect } = input;
  if (sourceSide && sourceRect && polylineHitsRect(path, sourceRect)) {
    const stub = input.sourceStub ?? LEAD_IN;
    const around = aroundSource(source, target, targetSide, sourceSide, sourceRect, stub, obstacles, margin);
    if (around) return around;
  }
  return path;
}

// A route that would cross its own source card, re-planned from a stub outside
// the anchored source side to the target's lead-in point, with the source card
// as one more obstacle. The grid search then turns away from the card (out,
// along, back in) rather than doubling back through it. Null when no such path
// exists, so the caller keeps the legacy route.
function aroundSource(
  source: Point,
  target: Point,
  targetSide: PortSide,
  sourceSide: PortSide,
  sourceRect: Rect,
  stub: number,
  obstacles: Rect[],
  margin: number,
): Point[] | null {
  const out = leadInPoint(source, sourceSide, stub);
  const lead = leadInPoint(target, targetSide, LEAD_IN);
  const routed = routeViaGrid(out, lead, [...obstacles, sourceRect], margin);
  return routed ? simplify([source, ...routed, target]) : null;
}

// The lead-in point one stub outside the target along the anchored side's
// outward normal — the final segment runs from here straight into the target,
// so the arrow arrives perpendicular to `side` from outside the card (#175).
function leadInPoint(target: Point, side: PortSide, stub: number): Point {
  switch (side) {
    case "right":
      return { x: target.x + stub, y: target.y };
    case "top":
      return { x: target.x, y: target.y - stub };
    case "bottom":
      return { x: target.x, y: target.y + stub };
    case "left":
    default:
      return { x: target.x - stub, y: target.y };
  }
}

// The bend-minimal orthogonal connection. The route leaves the source
// horizontally (output dots sit on the right by convention) and arrives
// perpendicular to `targetSide` from outside the target card. `left` keeps the
// legacy "HVH" step (vertical at the horizontal midpoint); the other sides turn
// in via a lead-in stub so the arrow approaches the anchored side rather than
// crossing the body. `simplify` collapses the redundant vertices — and, being
// monotonicity-aware, preserves an intentional lead-in overshoot when the
// source sits on the far side of the anchored edge.
function rawPath(source: Point, target: Point, targetSide: PortSide = "left"): Point[] {
  if (targetSide === "left") {
    const midX = (source.x + target.x) / 2;
    return simplify([
      source,
      { x: midX, y: source.y },
      { x: midX, y: target.y },
      target,
    ]);
  }
  const lead = leadInPoint(target, targetSide, LEAD_IN);
  if (targetSide === "right") {
    // Final segment horizontal into the right edge: leave horizontally, jog
    // vertically at the lead-in column, then run horizontally into the target.
    return simplify([source, { x: lead.x, y: source.y }, lead, target]);
  }
  // top / bottom: final segment vertical into the top/bottom edge — leave
  // horizontally into the target column, then run vertically through the lead-in
  // into the target.
  return simplify([source, { x: target.x, y: source.y }, lead, target]);
}

// Drops consecutive duplicate points and collinear midpoints so a straight run
// stays a single segment.
function dedupe(points: Point[]): Point[] {
  const out: Point[] = [];
  for (const p of points) {
    const last = out[out.length - 1];
    if (last && Math.abs(last.x - p.x) < 1e-6 && Math.abs(last.y - p.y) < 1e-6) {
      continue;
    }
    out.push(p);
  }
  return out;
}

// Removes interior vertices that lie on a straight run *between* their
// neighbours (three collinear, monotonic points collapse to two), keeping the
// polyline minimal. A collinear vertex that overshoots — the path doubles back
// on itself, as a lead-in stub does when the source sits on the far side of the
// anchored edge (#175) — is kept, since dropping it would erase the detour and
// send the arrow straight across the body.
function simplify(points: Point[]): Point[] {
  const pts = dedupe(points);
  if (pts.length <= 2) return pts;
  const out: Point[] = [pts[0]];
  for (let i = 1; i < pts.length - 1; i++) {
    const prev = out[out.length - 1];
    const cur = pts[i];
    const next = pts[i + 1];
    const collinearX =
      Math.abs(prev.x - cur.x) < 1e-6 && Math.abs(cur.x - next.x) < 1e-6;
    const collinearY =
      Math.abs(prev.y - cur.y) < 1e-6 && Math.abs(cur.y - next.y) < 1e-6;
    // `cur` is between its neighbours when they fall on opposite sides of it
    // along the shared axis (product <= 0). Only then is it a redundant
    // straight-run vertex; an overshoot (both neighbours on one side) is a real
    // turn and must survive.
    if (collinearX && (prev.y - cur.y) * (next.y - cur.y) <= 0) continue;
    if (collinearY && (prev.x - cur.x) * (next.x - cur.x) <= 0) continue;
    out.push(cur);
  }
  out.push(pts[pts.length - 1]);
  return out;
}

// True if an axis-aligned segment passes through the open interior of `rect`.
// Endpoints lying exactly on an edge do not count as a crossing.
function segmentHitsRect(a: Point, b: Point, rect: Rect): boolean {
  const x0 = rect.x;
  const x1 = rect.x + rect.width;
  const y0 = rect.y;
  const y1 = rect.y + rect.height;
  const segMinX = Math.min(a.x, b.x);
  const segMaxX = Math.max(a.x, b.x);
  const segMinY = Math.min(a.y, b.y);
  const segMaxY = Math.max(a.y, b.y);
  const overlapsX = segMaxX > x0 + 1e-6 && segMinX < x1 - 1e-6;
  const overlapsY = segMaxY > y0 + 1e-6 && segMinY < y1 - 1e-6;
  return overlapsX && overlapsY;
}

function polylineHitsRect(points: Point[], rect: Rect): boolean {
  for (let i = 1; i < points.length; i++) {
    if (segmentHitsRect(points[i - 1], points[i], rect)) return true;
  }
  return false;
}

// Builds a coarse routing grid from the endpoints plus each obstacle's
// margin-expanded edges, then A*-searches orthogonal moves between grid
// vertices, forbidding any move that crosses an obstacle interior. The grid
// lines are exactly the coordinates a route would want to turn on, so the
// search stays small while still threading the gaps between nodes.
function routeViaGrid(
  source: Point,
  target: Point,
  obstacles: Rect[],
  margin: number,
): Point[] | null {
  const xs = new Set<number>([source.x, target.x]);
  const ys = new Set<number>([source.y, target.y]);
  for (const o of obstacles) {
    xs.add(o.x - margin);
    xs.add(o.x + o.width + margin);
    ys.add(o.y - margin);
    ys.add(o.y + o.height + margin);
  }
  const gridX = [...xs].sort((p, q) => p - q);
  const gridY = [...ys].sort((p, q) => p - q);

  const ix = (x: number) => gridX.findIndex((v) => Math.abs(v - x) < 1e-6);
  const iy = (y: number) => gridY.findIndex((v) => Math.abs(v - y) < 1e-6);

  const start = { cx: ix(source.x), cy: iy(source.y) };
  const goal = { cx: ix(target.x), cy: iy(target.y) };
  if (start.cx < 0 || start.cy < 0 || goal.cx < 0 || goal.cy < 0) return null;

  // A search state is a cell AND the direction it was entered by: the turn
  // penalty depends on it, so two equal-cost arrivals at one cell are not
  // interchangeable. Keyed by cell alone, the first to arrive won and the other
  // was dropped even when it was the one that could go on straight — a route
  // turning back towards its source card for one more bend (#942 FP).
  type Dir = "x" | "y" | null;
  const key = (cx: number, cy: number, dir: Dir) => `${cx},${cy},${dir}`;
  const at = (cx: number, cy: number): Point => ({ x: gridX[cx], y: gridY[cy] });

  const moveBlocked = (a: Point, b: Point): boolean =>
    obstacles.some((o) => segmentHitsRect(a, b, o));

  // A* with Manhattan heuristic; turn penalty keeps the path from zig-zagging
  // when a straighter route of equal length exists.
  interface State {
    cx: number;
    cy: number;
    dir: Dir;
  }
  const startState: State = { ...start, dir: null };
  const open: (State & { f: number; g: number })[] = [{ ...startState, f: 0, g: 0 }];
  const best = new Map<string, number>([[key(start.cx, start.cy, null), 0]]);
  const came = new Map<string, State>();

  const heuristic = (cx: number, cy: number) =>
    Math.abs(gridX[cx] - target.x) + Math.abs(gridY[cy] - target.y);

  while (open.length > 0) {
    open.sort((a, b) => a.f - b.f);
    const cur = open.shift()!;
    if (cur.g > (best.get(key(cur.cx, cur.cy, cur.dir)) ?? Infinity)) continue;
    if (cur.cx === goal.cx && cur.cy === goal.cy) {
      return reconstruct(came, cur, key, at, source, target);
    }
    const neighbours: State[] = [
      { cx: cur.cx + 1, cy: cur.cy, dir: "x" },
      { cx: cur.cx - 1, cy: cur.cy, dir: "x" },
      { cx: cur.cx, cy: cur.cy + 1, dir: "y" },
      { cx: cur.cx, cy: cur.cy - 1, dir: "y" },
    ];
    for (const n of neighbours) {
      if (n.cx < 0 || n.cx >= gridX.length || n.cy < 0 || n.cy >= gridY.length) {
        continue;
      }
      const from = at(cur.cx, cur.cy);
      const to = at(n.cx, n.cy);
      if (moveBlocked(from, to)) continue;
      const stepCost = Math.abs(to.x - from.x) + Math.abs(to.y - from.y);
      const turnPenalty = cur.dir && cur.dir !== n.dir ? 1 : 0;
      const g = cur.g + stepCost + turnPenalty;
      const nk = key(n.cx, n.cy, n.dir);
      if (g < (best.get(nk) ?? Infinity)) {
        best.set(nk, g);
        came.set(nk, { cx: cur.cx, cy: cur.cy, dir: cur.dir });
        open.push({ ...n, g, f: g + heuristic(n.cx, n.cy) });
      }
    }
  }
  return null;
}

function reconstruct(
  came: Map<string, { cx: number; cy: number; dir: "x" | "y" | null }>,
  goal: { cx: number; cy: number; dir: "x" | "y" | null },
  key: (cx: number, cy: number, dir: "x" | "y" | null) => string,
  at: (cx: number, cy: number) => Point,
  source: Point,
  target: Point,
): Point[] {
  const cells: { cx: number; cy: number }[] = [{ cx: goal.cx, cy: goal.cy }];
  let cur = came.get(key(goal.cx, goal.cy, goal.dir));
  while (cur) {
    cells.push({ cx: cur.cx, cy: cur.cy });
    cur = came.get(key(cur.cx, cur.cy, cur.dir));
  }
  cells.reverse();
  const pts = cells.map((c) => at(c.cx, c.cy));
  // Pin the exact endpoints (the grid snaps them, but they should be verbatim).
  if (pts.length > 0) {
    pts[0] = source;
    pts[pts.length - 1] = target;
  }
  return simplify(pts);
}
