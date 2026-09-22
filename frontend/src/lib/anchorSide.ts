import type { EdgeAnchor, NodeType, PortSide } from "../types";
import type { Point } from "./orthogonalRouter";
import { snapToGrid } from "./wiringGrid";

/** An axis-aligned rectangle in canvas (flow) coordinates. */
export interface AnchorRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The four sides an emergent body anchor can land on (#168). */
export const ANCHOR_SIDES: readonly PortSide[] = ["left", "right", "top", "bottom"];

/**
 * Whether a node uses emergent inputs (ADR-0011 / #149, #168): incoming edges
 * land anywhere on the node body and anchor on the side nearest the drop point,
 * rather than binding to a declared, fixed-side input handle.
 *
 * The work-node type (`agent`) is emergent. This is keyed
 * on the node TYPE, not the declared input count: the #149 migration that drops
 * declared inputs was never carried through to node creation or the on-disk
 * pipeline YAMLs, so a work node frequently still carries a single vestigial
 * `in` input. Keying drop-anchoring on `inputs.length` therefore mis-classified
 * every such node as a fixed-side declared port and forced its arrows to the
 * left (the #175 bug). Keying on type makes both already-migrated (0-input) and
 * legacy (1-input `in`) work nodes anchor by drop.
 *
 * `start` has no inputs; `end` (declared `result`) and structural `merge` keep
 * their declared, fixed-side ports and are never re-anchored by drop position.
 */
export function isEmergentInputNode(type: NodeType): boolean {
  // #248: a `script` node consumes whole artifacts by edge like a work node, so
  // its inputs are emergent too — anchor incoming edges to the body by drop.
  return type === "agent" || type === "script";
}

/**
 * The xyflow handle id of the body-covering target handle on a given side
 * (#168). The EditNode renders one such handle per side; an incoming edge binds
 * to the one for its chosen `target_side` so the arrow anchors and routes from
 * that side. Distinct from declared port names (which never use this prefix).
 */
export function anchorHandleId(side: PortSide): string {
  return `__anchor:${side}`;
}

/** The side encoded in an {@link anchorHandleId}, or null for a non-anchor id. */
export function sideFromAnchorHandle(handleId: string | null | undefined): PortSide | null {
  if (!handleId) return null;
  const m = /^__anchor:(left|right|top|bottom)$/.exec(handleId);
  return m ? (m[1] as PortSide) : null;
}

/**
 * Whether a drop that landed on the handle `handleId` should anchor by drop
 * position (#168). Only an emergent body anchor handle does; a declared input
 * (End's `result`) or a structural port (merge `branches`, loop `in`) keeps its
 * fixed declared side and must be left untouched (AC: declared ports unaffected).
 */
export function anchorsByDropOnBody(handleId: string | null | undefined): boolean {
  return sideFromAnchorHandle(handleId) != null;
}

/**
 * Chooses the side of a target card nearest a drop point (issue #168). When an
 * edge is dropped on a node's body (emergent input, ADR-0011 / #149), the
 * incoming arrow anchors on the side the user aimed at — "the arrow goes where
 * you drop it" (#219) — not always the left.
 *
 * The rule is the side a ray from the card centre through the drop point would
 * exit: split the card into four triangular sectors by its diagonals and pick
 * the sector the drop falls in. We measure the drop's offset from the centre
 * NORMALISED by each half-extent, so the choice is aspect-ratio-aware.
 *
 * The earlier perpendicular-distance-to-each-edge metric (#219) was wrong on a
 * non-square card: the default work node is short and wide (~160x35), so nearly
 * any interior drop sits closer to the top/bottom edges than to the left/right
 * ones and spuriously anchored top/bottom. Normalising by the half-extents
 * removes that bias — a drop in the left third of a wide card resolves to left,
 * not top.
 *
 * Ties resolve in left, right, top, bottom order: horizontal sides win over
 * vertical ones (`|dx| >= |dy|`) and left wins over right (`dx <= 0`), so a
 * dead-centre drop keeps the legacy left default. (That default is also enforced
 * by the caller, which persists a chosen side only when it is NOT left — see
 * `EditCanvas.onConnectEnd`.)
 */
export function chooseAnchorSide(
  drop: { x: number; y: number },
  rect: AnchorRect,
): PortSide {
  const halfW = rect.width / 2;
  const halfH = rect.height / 2;
  const cx = rect.x + halfW;
  const cy = rect.y + halfH;
  // Direction from the centre to the drop, scaled to the card's own aspect ratio.
  const dx = halfW > 0 ? (drop.x - cx) / halfW : 0;
  const dy = halfH > 0 ? (drop.y - cy) / halfH : 0;

  if (Math.abs(dx) >= Math.abs(dy)) {
    return dx > 0 ? "right" : "left";
  }
  return dy > 0 ? "bottom" : "top";
}

// ── Per-edge anchors (#844) ─────────────────────────────────────────────────
//
// WHERE on a card's side a wire leaves from and lands on. Until #844 a wire left
// the middle of the side it was dragged from and landed on the middle of the
// nearest side — so two wires out of the same card overlapped, and the gesture's
// precision was thrown away. An anchor keeps the exact spot: the side (the coarse
// `target_side` that already existed), plus an offset ALONG that side.
//
// Layout, like `target_side` and `waypoints`: it belongs in the pipeline file and
// outside the semantic diff (`lib/layoutFields.ts`).

/** How close to a corner an anchor may sit. A wire leaving exactly on the corner
 *  reads as leaving from either of two sides. */
const CORNER_MARGIN = 10;

/** `true` when the side runs horizontally (so the offset is an x distance). */
export function sideIsHorizontal(side: PortSide): boolean {
  return side === "top" || side === "bottom";
}

/** The length of the side an offset runs along. */
export function sideLength(rect: AnchorRect, side: PortSide): number {
  return sideIsHorizontal(side) ? rect.width : rect.height;
}

/** The absolute flow point of an anchor on `rect`; a null anchor is the middle of
 *  the fallback `side`. */
export function anchorPoint(
  rect: AnchorRect,
  anchor: EdgeAnchor | null | undefined,
  side: PortSide,
): Point {
  const s = anchor?.side ?? side;
  const along = anchor ? clampOffset(anchor.offset, rect, s) : sideLength(rect, s) / 2;
  switch (s) {
    case "top":
      return { x: rect.x + along, y: rect.y };
    case "bottom":
      return { x: rect.x + along, y: rect.y + rect.height };
    case "left":
      return { x: rect.x, y: rect.y + along };
    case "right":
      return { x: rect.x + rect.width, y: rect.y + along };
  }
}

function clampOffset(offset: number, rect: AnchorRect, side: PortSide): number {
  const len = sideLength(rect, side);
  // A card narrower than two margins has a single legal anchor: its middle.
  const margin = Math.min(CORNER_MARGIN, len / 2);
  return Math.max(margin, Math.min(len - margin, offset));
}

/**
 * The anchor for a press/drop at `point` on the given `side` of `rect`: the point
 * is projected onto the side, the resulting absolute coordinate is snapped to the
 * wiring grid (so a wire leaves on a grid line and its first segment is already
 * aligned), and the offset is clamped clear of the corners.
 *
 * `step = 0` skips the snap — that is the target side, where the drop is taken
 * per-pixel so the arrow lands exactly where it was aimed.
 */
export function anchorFromPoint(
  point: Point,
  rect: AnchorRect,
  side: PortSide,
  step: number,
): EdgeAnchor {
  const horizontal = sideIsHorizontal(side);
  const raw = horizontal ? point.x : point.y;
  const snapped =
    step > 0
      ? horizontal
        ? snapToGrid({ x: raw, y: 0 }, { x: 0, y: 0 }, step).x
        : snapToGrid({ x: 0, y: raw }, { x: 0, y: 0 }, step).y
      : raw;
  const origin = horizontal ? rect.x : rect.y;
  return { side, offset: clampOffset(snapped - origin, rect, side) };
}

/**
 * The anchor a drop at `point` on `rect` resolves to: the side it was aimed at,
 * and the position ALONG that side it was aimed at. `target_side` stays the
 * coarse layout field it always was; the offset is what #844 adds.
 *
 * The SIDE comes from {@link chooseAnchorSide} — the centre-ray rule — and not
 * from "which border is nearest". That distinction is the whole of #219: the
 * default work card is short and wide (~160x35), so nearly any interior drop is
 * closer to the top or bottom border than to the left or right one, and a
 * nearest-border rule anchors almost every arrow top/bottom. The offset is taken
 * PER PIXEL (no grid snap): the drop point is the intent.
 */
export function dropAnchor(point: Point, rect: AnchorRect): EdgeAnchor {
  return anchorFromPoint(point, rect, chooseAnchorSide(point, rect), 0);
}

// ── Perpendicular entry and exit ────────────────────────────────────────────
//
// An arrowhead only reads as « arriving here » if the wire runs straight into the
// side it lands on. Otherwise the last leg is whatever the trace happened to be
// doing, so the arrow can slide along the border or meet it at a corner.
//
// The rule: the FIRST segment leaves the source side perpendicular to it, and the
// LAST segment enters the target side perpendicular to it — turn first, then run
// straight in — with a minimum length so the straight entry is visible.

/** The axis a wire must travel on to cross `side` head-on. */
export function perpAxis(side: PortSide): "horizontal" | "vertical" {
  return sideIsHorizontal(side) ? "vertical" : "horizontal";
}

/** Unit step pointing OUT of the card, away from `side`. */
export function outwardDelta(side: PortSide): Point {
  switch (side) {
    case "left":
      return { x: -1, y: 0 };
    case "right":
      return { x: 1, y: 0 };
    case "top":
      return { x: 0, y: -1 };
    case "bottom":
      return { x: 0, y: 1 };
  }
}

/**
 * Minimum length of the perpendicular leg: one grid cell, never under 16px. At a
 * 40px grid the leg is a cell; if the constant is ever lowered, 16px is the floor
 * below which a straight entry stops being legible.
 */
export function landingLeg(step: number): number {
  return Math.max(16, step);
}

/** The point one leg OUTSIDE the border, from which the wire runs straight in. */
export function approachPoint(anchor: Point, side: PortSide, leg: number): Point {
  const d = outwardDelta(side);
  return { x: anchor.x + d.x * leg, y: anchor.y + d.y * leg };
}

/**
 * The connector from `from` (the last committed point) to `anchor` on `side`: an
 * orthogonal L whose LAST segment is the perpendicular leg. Never diagonal, never
 * a leg running parallel to the side.
 *
 * Built backwards from the anchor: the wire must reach `approach` — one leg out
 * from the border, aligned with the anchor on the crossing axis — and it must
 * arrive there moving PARALLEL to the side, so the only turn left is the one into
 * the card. Hence the intermediate point, which the collinear merge drops whenever
 * the approach is already lined up.
 */
export function landingConnector(
  from: Point,
  anchor: Point,
  side: PortSide,
  leg: number,
): Point[] {
  const approach = approachPoint(anchor, side, leg);
  // `left`/`right`: the leg is horizontal, so we must enter `approach` vertically
  // ⇒ travel horizontally first. `top`/`bottom` is the transpose.
  const bend = sideIsHorizontal(side)
    ? { x: from.x, y: approach.y }
    : { x: approach.x, y: from.y };
  return dedupeCollinear([from, bend, approach, anchor]);
}

/**
 * Re-imposes the two rules on a finished polyline — used when persisting a drawn
 * edge, when re-rendering after a node move, and after a segment drag.
 *
 * Built by ASSEMBLY rather than by patching points in place. Overwriting the
 * endpoint-adjacent waypoints knocks the segment one further in out of square,
 * and re-patching that one knocks out the next: the two ends come out
 * perpendicular and the middle a sawtooth of diagonals. Here the two approach
 * points are *inserted* as fixed points and `squareUp` repairs the rest by
 * inserting bends, never by moving anything. So nothing already placed can be
 * disturbed, and the two legs are perpendicular by construction instead of by
 * repeated correction.
 */
export function enforcePerpendicularEnds(
  points: Point[],
  sourceSide: PortSide,
  targetSide: PortSide,
  leg: number,
): Point[] {
  if (points.length < 2) return points;
  const src = points[0];
  const tgt = points[points.length - 1];
  // Interior points sitting ON an endpoint are dropped: `dragSegmentOnGrid`
  // inserts a bend beside a pinned endpoint, which can land exactly on it, and
  // once the approach point is inserted in front of such a waypoint the wire
  // leaves the card, comes straight back to where it started, and only then sets
  // off — a visible spur at the source.
  const onEndpoint = (p: Point) =>
    (Math.abs(p.x - src.x) < TOL && Math.abs(p.y - src.y) < TOL) ||
    (Math.abs(p.x - tgt.x) < TOL && Math.abs(p.y - tgt.y) < TOL);
  const interior = points.slice(1, -1).filter((p) => !onEndpoint(p));
  const sA = approachPoint(src, sourceSide, leg);
  const tA = approachPoint(tgt, targetSide, leg);

  // Merge the middle FIRST, and never across the two approach points. When the
  // run arriving at the target approach is itself along the leg's axis — which
  // happens once a node is dragged past its own waypoints — a merge applied
  // afterwards swallows the approach point, and the surviving segment runs from
  // INSIDE the card outwards: the leg is still perpendicular and the path still
  // square, the arrowhead simply points out of the card instead of into it.
  // `dedupeCollinear` preserves the first and last element of whatever it is
  // given, so merging the middle alone keeps both legs intact by construction.
  const mid = dedupeCollinear([sA, ...interior, tA]);
  // `squareUp` only inserts, so the four pinned points survive.
  const squared = squareUp([src, ...mid, tgt]);
  // …and then merge the middle once more, because a bend `squareUp` inserted can
  // land on a straight run it has just completed. Leaving it in would be
  // harmless on screen and poisonous in the file: the redundant point is
  // PERSISTED, the next render merges it away, and what is reloaded no longer
  // matches what was saved. Enforcement has to be a fixed point — enforce
  // ∘ enforce = enforce — or every open/save cycle rewrites the route.
  //
  // `src`/`tgt` are pinned to their cards, and `squareUp` never inserts between
  // an endpoint and its own approach point (that segment is already square), so
  // the two approach points are still at index 1 and len-2. Merging strictly
  // between the endpoints therefore keeps both legs whole.
  return [
    squared[0],
    ...dedupeCollinear(squared.slice(1, -1)),
    squared[squared.length - 1],
  ];
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

const TOL = 1e-6;

/**
 * Local collinear merge. Endpoints and the two approach points survive by
 * construction: dropping a mid-run point never changes where a run starts or
 * ends, so a merged straight run into the border is still perpendicular.
 */
function dedupeCollinear(points: Point[]): Point[] {
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

/** The xyflow handle id of the rim drag-source strip on `side` (#844). */
export function rimHandleId(side: PortSide): string {
  return `rim-${side}`;
}

/** The side encoded in a {@link rimHandleId}, or null for a non-rim id. */
export function sideFromRimHandle(handleId: string | null | undefined): PortSide | null {
  if (!handleId) return null;
  const m = /^rim-(left|right|top|bottom)$/.exec(handleId);
  return m ? (m[1] as PortSide) : null;
}
