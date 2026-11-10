/**
 * The clean-up of an orthogonal wire (#844): a segment drag that keeps its run,
 * squaring, collinear merges, spurs and border runs dropped, the part outside a
 * card, and the waypoints worth storing.
 */
import type { Point } from "./orthogonalRouter";
import { dragSegmentOnGrid } from "./wiringGrid";
import type { AnchorRect } from "./anchorSide";

/**
 * A segment drag whose dragged run survives enforcement.
 *
 * `dragSegmentOnGrid` knows nothing of the target card, so it can put the run
 * where no route can keep it — typically the neighbour magnet, which lines the
 * run up with the landing approach and so parks it straight across the target.
 * Enforcement then has to re-lay the wire without it, and the run the user is
 * holding jumps away from the pointer, back above the card (#844 FP iter-5,
 * finding 2). So the magnet is dropped for that position and the plain grid tried
 * instead; `keepsRun: false` means neither worked, and the caller should hold the
 * last position that did rather than draw the jump.
 *
 * With `targetRect`, a position that lays the wire ALONG one of the card's
 * borders does not count either: the run is kept, but drawn over the card's
 * outline, where it reads as neither inside nor out.
 */
export function dragSegmentKeepingRun(
  points: Point[],
  segmentIndex: number,
  coord: number,
  grid: { origin: Point; step: number; free: boolean },
  enforce: (points: Point[]) => Point[],
  targetRect?: AnchorRect,
): { moved: Point[]; enforced: Point[]; keepsRun: boolean } {
  // Dragging the first segment inserts a bend beside the pinned source, which
  // shifts the dragged run one index along.
  const at = segmentIndex === 0 ? 1 : segmentIndex;
  let last: { moved: Point[]; enforced: Point[]; keepsRun: boolean } | null = null;
  for (const magnet of grid.free ? [false] : [true, false]) {
    const moved = dragSegmentOnGrid(points, segmentIndex, coord, { ...grid, magnet });
    const enforced = enforce(moved);
    const [a, b] = [moved[at], moved[at + 1]];
    const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    const keepsRun =
      [a, mid, b].every((p) => onPolyline(p, enforced)) &&
      !(targetRect && runsAlongBorder(enforced.slice(0, -1), targetRect));
    last = { moved, enforced, keepsRun };
    if (keepsRun) break;
  }
  return last!;
}

/**
 * Makes every segment axis-aligned by INSERTING a bend into each diagonal,
 * continuing the incoming direction first (« straight, then turn »). No existing
 * point ever moves, so pinned endpoints and approach points survive untouched.
 *
 * Insertion is why this is safe where moving points was not: a collinear leftover
 * is merged away afterwards, and merging a run never changes where that run starts
 * or ends — so a leg that was perpendicular stays perpendicular, it only gets
 * longer.
 */
export function squareUp(points: Point[]): Point[] {
  if (points.length < 2) return points;
  const out: Point[] = [{ ...points[0] }];
  for (let i = 1; i < points.length; i++) {
    const a = out[out.length - 1];
    const b = points[i];
    const square = Math.abs(a.x - b.x) < TOL || Math.abs(a.y - b.y) < TOL;
    if (!square) {
      const before = out[out.length - 2];
      const incomingHorizontal = before ? Math.abs(before.y - a.y) < TOL : false;
      // Carry on the way the wire was already going, then corner — UNLESS
      // carrying on would mean reversing. Continuing blindly puts a spur on the
      // source: the leg leaves the card rightwards, the next waypoint is back to
      // the left, and « continue horizontally » walks straight back over the leg
      // it just drew. When the next point lies behind, turn first.
      const reverses = before
        ? incomingHorizontal
          ? Math.sign(b.x - a.x) === -Math.sign(a.x - before.x)
          : Math.sign(b.y - a.y) === -Math.sign(a.y - before.y)
        : false;
      const continueAxisHorizontal = incomingHorizontal !== reverses;
      out.push(continueAxisHorizontal ? { x: b.x, y: a.y } : { x: a.x, y: b.y });
    }
    out.push({ ...b });
  }
  return out;
}

export const TOL = 1e-6;

/**
 * Local collinear merge. Endpoints and the two approach points survive by
 * construction: dropping a mid-run point never changes where a run starts or
 * ends, so a merged straight run into the border is still perpendicular.
 */
export function dedupeCollinear(points: Point[]): Point[] {
  const out: Point[] = [];
  for (const p of points) {
    const last = out[out.length - 1];
    if (last && Math.abs(last.x - p.x) < TOL && Math.abs(last.y - p.y) < TOL) continue;
    out.push({ ...p });
    while (out.length >= 3) {
      const [a, b, c] = out.slice(-3);
      const straight =
        (Math.abs(a.y - b.y) < TOL && Math.abs(b.y - c.y) < TOL) ||
        (Math.abs(a.x - b.x) < TOL && Math.abs(b.x - c.x) < TOL);
      if (!straight) break;
      out.splice(out.length - 2, 1);
    }
  }
  return out;
}

/** Whether a segment of `points` runs ON one of `rect`'s borders, over a
 *  stretch of it (a wire crossing a border at a point does not count). */
function runsAlongBorder(points: Point[], rect: AnchorRect): boolean {
  const [x0, x1, y0, y1] = [rect.x, rect.x + rect.width, rect.y, rect.y + rect.height];
  const overlaps = (lo: number, hi: number, e0: number, e1: number) =>
    Math.min(lo, hi) < e1 - TOL && Math.max(lo, hi) > e0 + TOL;
  for (let i = 1; i < points.length; i++) {
    const [a, b] = [points[i - 1], points[i]];
    const horizontal = Math.abs(a.y - b.y) < TOL;
    const vertical = Math.abs(a.x - b.x) < TOL;
    if (
      horizontal &&
      (Math.abs(a.y - y0) < TOL || Math.abs(a.y - y1) < TOL) &&
      overlaps(a.x, b.x, x0, x1)
    ) {
      return true;
    }
    if (
      vertical &&
      (Math.abs(a.x - x0) < TOL || Math.abs(a.x - x1) < TOL) &&
      overlaps(a.y, b.y, y0, y1)
    ) {
      return true;
    }
  }
  return false;
}

/** Whether `p` lies on one of the axis-aligned segments of `points`. */
export function onPolyline(p: Point, points: Point[]): boolean {
  for (let i = 1; i < points.length; i++) {
    const [a, b] = [points[i - 1], points[i]];
    const within = (v: number, e0: number, e1: number) =>
      v >= Math.min(e0, e1) - TOL && v <= Math.max(e0, e1) + TOL;
    if (within(p.x, a.x, b.x) && within(p.y, a.y, b.y)) return true;
  }
  return false;
}

/**
 * Collinear merge restricted to REVERSALS: a point where the wire runs out along
 * an axis and straight back is dropped; a point in the middle of a run going one
 * way is kept. The first and last element always survive.
 */
export function dropSpurs(points: Point[]): Point[] {
  const out: Point[] = [];
  for (const p of points) {
    const last = out[out.length - 1];
    if (last && Math.abs(last.x - p.x) < TOL && Math.abs(last.y - p.y) < TOL) continue;
    out.push({ ...p });
    while (out.length >= 3) {
      const [a, b, c] = out.slice(-3);
      const vertical = Math.abs(a.x - b.x) < TOL && Math.abs(b.x - c.x) < TOL;
      const horizontal = Math.abs(a.y - b.y) < TOL && Math.abs(b.y - c.y) < TOL;
      const reverses = vertical
        ? (b.y - a.y) * (c.y - b.y) < 0
        : horizontal && (b.x - a.x) * (c.x - b.x) < 0;
      if (!reverses) break;
      out.splice(out.length - 2, 1);
    }
  }
  return out;
}

/**
 * Drops trailing points that have already entered the target's neighbourhood —
 * the card inflated by one leg.
 *
 * The trace keeps growing on the grid right up to the moment xyflow reports the
 * pointer as being over a node, by which time the tip can already be ON the
 * border. Building the landing connector from that tip makes the wire double back
 * out of the card and in again — a visible spur with the arrowhead behind it.
 * Clipping first means the connector starts from the last point that was genuinely
 * outside.
 */
export function clipOutside(points: Point[], rect: AnchorRect, margin: number): Point[] {
  const inside = (p: Point) =>
    p.x >= rect.x - margin &&
    p.x <= rect.x + rect.width + margin &&
    p.y >= rect.y - margin &&
    p.y <= rect.y + rect.height + margin;
  let end = points.length;
  while (end > 1 && inside(points[end - 1])) end--;
  return points.slice(0, end);
}

/**
 * The storable waypoints of an enforced path: everything strictly BETWEEN the two
 * perpendicular legs.
 *
 * An enforced path is `[src, sourceApproach, ...mid, targetApproach, tgt]`. The
 * two approach points are derived from the anchors, so storing them is storing a
 * duplicate — and a duplicate that drifts: drag the segment next to one and the
 * stored copy no longer matches the recomputed one, so the next render routes
 * *around* the stale copy, adding a jog and sometimes a spur. Persist `mid` only
 * and let enforcement re-derive the legs every time.
 */
export function storableWaypoints(enforced: Point[]): Point[] {
  return enforced.length <= 4 ? [] : enforced.slice(2, -2).map((p) => ({ ...p }));
}

/** `a → b → c` on one line, `c` back between `a` and `b`: a spur on the leg
 *  `a → b`. A `c` past `a` runs back through the card, which is
 *  {@link aroundSource}'s to re-lay. */
function spursBack(a: Point, b: Point, c: Point): boolean {
  const within = (lo: number, hi: number, v: number) => v > Math.min(lo, hi) && v < Math.max(lo, hi);
  if (a.y === b.y && b.y === c.y) return within(a.x, b.x, c.x);
  if (a.x === b.x && b.x === c.x) return within(a.y, b.y, c.y);
  return false;
}

/**
 * Folds the spur a bend closer to its card than one leg leaves on that leg: the
 * wire ran out to the approach point and came back to the bend before it turned
 * (`308 → 338 → 319` out of a right anchor, FP of CORE-227). The run after the
 * spur slides onto the approach point instead, so the leg keeps its length and
 * the wire turns where it ends. The fold leaves nothing to fold again, so
 * enforcement stays a fixed point.
 */
export function withoutSourceLegSpur(points: Point[]): Point[] {
  let out = points;
  // The run after the spur is perpendicular to the leg: it slides along it.
  const onto = (p: Point, approach: Point, legHorizontal: boolean): Point =>
    legHorizontal ? { x: approach.x, y: p.y } : { x: p.x, y: approach.y };
  if (out.length >= 5 && spursBack(out[0], out[1], out[2])) {
    const approach = out[1];
    out = [out[0], approach, onto(out[3], approach, approach.y === out[0].y), ...out.slice(4)];
  }
  return out;
}
