/**
 * How a wire lands on its target card (#844): the perpendicular leg into the
 * anchor, the connector that reaches it around the card, and the enforcement
 * that keeps both ends of a stored route perpendicular to their borders.
 */
import type { PortSide } from "../shared/generated/PortSide";
import type { Point } from "./orthogonalRouter";
import type { AnchorRect } from "./anchorSide";
import { outwardDelta, sideIsHorizontal } from "./anchorSide";
import {
  TOL,
  clipOutside,
  dedupeCollinear,
  dropSpurs,
  onPolyline,
  squareUp,
  withoutSourceLegSpur,
} from "./polylineCleanup";

/**
 * Minimum length of the perpendicular leg: one grid cell of the size in effect
 * (20/30/40px, #877), never under 16px — the floor below which a straight entry
 * stops being legible, kept under the smallest size.
 */
export function landingLeg(step: number): number {
  return Math.max(16, step);
}

/**
 * The leg two FACING anchors can both have: the given one, or half the gap
 * between them when two full legs would overlap — the wire would otherwise run
 * past the target's border, back into the source card and out again, a line
 * through both cards.
 */
function meetingLeg(src: Point, tgt: Point, sourceSide: PortSide, targetSide: PortSide, leg: number): number {
  const gaps: Partial<Record<PortSide, Partial<Record<PortSide, number>>>> = {
    bottom: { top: tgt.y - src.y },
    top: { bottom: src.y - tgt.y },
    right: { left: tgt.x - src.x },
    left: { right: src.x - tgt.x },
  };
  const gap = gaps[sourceSide]?.[targetSide];
  return gap != null && gap > 0 && gap < 2 * leg ? gap / 2 : leg;
}

/** Strictly inside the card: a waypoint there is a leftover of a card since moved or grown. */
function insideCard(p: Point, rect: AnchorRect): boolean {
  return p.x > rect.x + TOL && p.x < rect.x + rect.width - TOL && p.y > rect.y + TOL && p.y < rect.y + rect.height - TOL;
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
 *
 * `rect` (the target card) makes the connector go AROUND it rather than through
 * it. The approach point sits one leg beyond the chosen side, so whenever the wire
 * arrives from any other side the plain L walks straight across the card body,
 * overshoots past the far border and doubles back in — invisible under an opaque
 * card, but persisted, and on show the moment edges are drawn above nodes. The
 * detour leaves by the nearest free corridor instead (#844, FP finding 2).
 */
export function landingConnector(
  from: Point,
  anchor: Point,
  side: PortSide,
  leg: number,
  rect?: AnchorRect,
): Point[] {
  const approach = approachPoint(anchor, side, leg);
  // `left`/`right`: the leg is horizontal, so we must enter `approach` vertically
  // ⇒ travel horizontally first. `top`/`bottom` is the transpose.
  const bend = sideIsHorizontal(side)
    ? { x: from.x, y: approach.y }
    : { x: approach.x, y: from.y };
  const direct = [from, bend, approach, anchor];
  if (!rect || !crossesRect(direct.slice(0, 3), rect)) return dedupeCollinear(direct);

  // Around the card: out to a corridor one leg clear of the nearer parallel
  // border, along it, then back in line with the approach. Every leg of this
  // shape stays outside the card given `from` is outside it — which is what
  // `clipOutside` guarantees the caller.
  const detour = sideIsHorizontal(side)
    ? (() => {
        const x = nearerCorridor(from.x, rect.x, rect.x + rect.width, leg);
        return [{ x, y: from.y }, { x, y: approach.y }];
      })()
    : (() => {
        const y = nearerCorridor(from.y, rect.y, rect.y + rect.height, leg);
        return [{ x: from.x, y }, { x: approach.x, y }];
      })();
  return dedupeCollinear([from, ...detour, approach, anchor]);
}

/** The free lane one leg beyond whichever of the two borders `v` is nearer. */
function nearerCorridor(v: number, low: number, high: number, leg: number): number {
  return v - low <= high - v ? low - leg : high + leg;
}

/** Whether any segment of an axis-aligned polyline runs through `rect`'s inside.
 *  Touching a border is not crossing it: a wire may run along a card's edge. */
function crossesRect(points: Point[], rect: AnchorRect): boolean {
  const x0 = rect.x;
  const x1 = rect.x + rect.width;
  const y0 = rect.y;
  const y1 = rect.y + rect.height;
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const spanLow = (lo: number, hi: number, e0: number, e1: number) =>
      Math.min(lo, hi) < e1 - TOL && Math.max(lo, hi) > e0 + TOL;
    if (spanLow(a.x, b.x, x0, x1) && spanLow(a.y, b.y, y0, y1)) return true;
  }
  return false;
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
 *
 * `targetRect` (the target card) keeps the wire from running THROUGH the card on
 * its way to the landing leg — see {@link aroundTarget}. `manual` says whether the
 * interior points are the user's (a manual route) or the router's (an auto one):
 * only the user's are worth preserving when the wire has to be re-laid.
 * `sourceRect` (the source card) keeps an AUTO route from running back through
 * the card it leaves — see {@link aroundSource}; a manual route is the user's.
 *
 * Two facing anchors closer than two legs share the gap ({@link meetingLeg});
 * an interior point inside either card, or on either leg, is then dropped: it
 * would send the wire back through a card or into a spur.
 */
export function enforcePerpendicularEnds(
  points: Point[],
  sourceSide: PortSide,
  targetSide: PortSide,
  leg: number,
  targetRect?: AnchorRect,
  { manual = true, sourceRect }: { manual?: boolean; sourceRect?: AnchorRect } = {},
): Point[] {
  if (points.length < 2) return points;
  const src = points[0];
  const tgt = points[points.length - 1];
  const meeting = meetingLeg(src, tgt, sourceSide, targetSide, leg);
  const close = meeting < leg;
  leg = meeting;
  // Interior points sitting ON an endpoint are dropped: `dragSegmentOnGrid`
  // inserts a bend beside a pinned endpoint, which can land exactly on it, and
  // once the approach point is inserted in front of such a waypoint the wire
  // leaves the card, comes straight back to where it started, and only then sets
  // off — a visible spur at the source.
  const onEndpoint = (p: Point) =>
    (Math.abs(p.x - src.x) < TOL && Math.abs(p.y - src.y) < TOL) ||
    (Math.abs(p.x - tgt.x) < TOL && Math.abs(p.y - tgt.y) < TOL);
  const sA = approachPoint(src, sourceSide, leg);
  const tA = approachPoint(tgt, targetSide, leg);
  // Between two close facing cards, nothing but the two legs fits: a waypoint
  // on a leg or inside a card is a leftover of the cards' older places.
  const stray = (p: Point) =>
    close &&
    ((sourceRect != null && insideCard(p, sourceRect)) ||
      (targetRect != null && insideCard(p, targetRect)) ||
      onPolyline(p, [src, sA]) ||
      onPolyline(p, [tA, tgt]));
  const interior = points.slice(1, -1).filter((p) => !onEndpoint(p) && !stray(p));

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
  const unfolded = squareUp([src, ...mid, tgt]);
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
  const merge = (pts: Point[]) => [pts[0], ...dedupeCollinear(pts.slice(1, -1)), pts[pts.length - 1]];
  // A spur on the source leg is folded on the MERGED route, where the run after
  // it is one segment: folded before, a bend `squareUp` inserted on that run
  // stayed behind and left a diagonal.
  const merged = merge(unfolded);
  const folded = withoutSourceLegSpur(merged);
  const squared = folded === merged ? unfolded : folded;
  const enforced = folded === merged ? merged : merge(folded);
  const landed = !targetRect
    ? enforced
    : manual
      ? aroundTarget(squared, enforced, targetSide, leg, targetRect)
      : aroundTarget(enforced, enforced, targetSide, leg, targetRect, { sidestep: false });
  return !manual && sourceRect ? aroundSource(landed, leg, sourceRect, targetRect) : landed;
}

/**
 * Re-lays an AUTO route that runs back through its own source card.
 *
 * The router plans as if every wire left by the right, and enforcement only
 * inserts bends: towards a target below and to the LEFT of its source — two
 * cards stacked in a column, the most common layout there is — the wire left by
 * the right, came straight back across the middle of the card it had just left
 * and only then went down. Drawn above the nodes (#845), its hit stroke sat on
 * the card's name, so a click on the node selected the edge instead.
 *
 * The segment crossing the card is pushed into the corridor beyond one of its
 * two parallel borders, whichever gives the shorter wire, provided the result
 * crosses neither card. Anything else — no crossing, or no clean detour — is
 * returned unchanged, so enforcement stays a fixed point.
 */
function aroundSource(
  path: Point[],
  leg: number,
  rect: AnchorRect,
  targetRect?: AnchorRect,
): Point[] {
  // Everything but the source leg, which starts ON the border by design.
  if (!crossesRect(path.slice(1), rect)) return path;
  const tidy = (p: Point[]) => [p[0], ...dedupeCollinear(p.slice(1, -1)), p[p.length - 1]];
  const clear = (p: Point[]) =>
    !crossesRect(p.slice(1), rect) && (!targetRect || !crossesRect(p.slice(0, -1), targetRect));
  const length = (p: Point[]) =>
    p.slice(1).reduce((sum, b, i) => sum + Math.abs(b.x - p[i].x) + Math.abs(b.y - p[i].y), 0);
  const detours = [false, true]
    .map((far) => tidy(sidestepCrossings(path, rect, leg, far)))
    .filter(clear);
  if (detours.length === 0) return path;
  return detours.reduce((best, detour) => (length(detour) < length(best) ? detour : best));
}

/**
 * Re-lays an enforced path around the target card when it runs through it.
 *
 * The router plans as if every wire left by the right, and enforcement only
 * INSERTS bends, so a wire leaving a card's bottom towards a target right below
 * it, arriving on its (default) left side, was squared down the middle of the
 * target, across it, out of its far border and back in: the leg into the card
 * then drew an arrowhead from empty space (#844 FP iter-3).
 *
 * Two repairs, the gentler first:
 * 1. A segment that merely PASSES through the card (both ends outside it) is
 *    pushed sideways into the nearer free corridor. Everything else — in
 *    particular a segment the user has just dragged past the card — stays where
 *    it was put, so the wire goes around the far side instead of jumping back
 *    against the pointer (#844 FP iter-4, finding 2).
 * 2. Otherwise the tail ends inside the card: keep the route up to the last
 *    point genuinely clear of it, then land with the same around-the-card
 *    connector the drawing gesture uses. The clip reads `squared` — the path
 *    BEFORE the final collinear merge — so the user's pins on a straight run
 *    still count as « clear » and the wire turns after the last of them, not at
 *    the source approach (#844 FP iter-4, finding 1).
 *
 * Both repairs exist to respect what the USER put there. An auto route has no
 * such intent — its interior points are the router's, planned as if the wire
 * left by the right, and a collinear router bend kept as a « pin » made the wire
 * climb back to its own source card and run along its border (#844 FP iter-5,
 * finding 1). So an auto route is handed in already merged, with `sidestep` off:
 * it only ever gets the truncate-and-land repair, from its last real corner.
 *
 * A fixed point like the rest of enforcement: a body already clear of the card
 * is returned untouched, so re-enforcing the stored waypoints reproduces the
 * route. A card too close to go around returns the square route unchanged.
 */
function aroundTarget(
  squared: Point[],
  enforced: Point[],
  targetSide: PortSide,
  leg: number,
  rect: AnchorRect,
  { sidestep = true }: { sidestep?: boolean } = {},
): Point[] {
  // Everything but the perpendicular leg, which ends ON the border by design.
  if (!crossesRect(enforced.slice(0, -1), rect)) return enforced;
  const clear = (path: Point[]) => !crossesRect(path.slice(0, -1), rect);
  const tidy = (path: Point[]) => [
    path[0],
    ...dedupeCollinear(path.slice(1, -1)),
    path[path.length - 1],
  ];

  // The square path with only its reversal spurs removed: a wire doubling back
  // on itself is noise, a pin on a straight run is intent.
  const path = [squared[0], ...dropSpurs(squared.slice(1, -1)), squared[squared.length - 1]];

  // Nearer corridor first. A candidate must still pass through every point of
  // the route: going round by the side the wire lands on can collapse the detour
  // onto the landing and silently drop the very segment the user placed.
  for (const far of sidestep ? [false, true] : []) {
    const shifted = tidy(sidestepCrossings(path, rect, leg, far));
    if (clear(shifted) && path.every((p) => onPolyline(p, shifted))) return shifted;
  }

  const tgt = path[path.length - 1];
  // Never clip into the source leg: the first two points are pinned.
  const kept = [...path.slice(0, 2), ...clipOutside(path.slice(2, -2), rect, leg - TOL)];
  let from = kept.length;
  // The kept prefix may still run into the card on its last segment.
  while (from > 2 && crossesRect(kept.slice(0, from), rect)) from--;
  const prefix = kept.slice(0, from);
  const connector = landingConnector(prefix[prefix.length - 1], tgt, targetSide, leg, rect);
  const rerouted = tidy([...prefix, ...connector.slice(1, -1), tgt]);
  // A card too close to the source to go around cleanly: keep the square route
  // rather than trade one crossing for another.
  return clear(rerouted) ? rerouted : enforced;
}

/**
 * Pushes every segment that runs straight THROUGH `rect` — both of its ends
 * outside the card — into the free corridor beyond the nearer parallel border
 * (the farther one when `far`): `a → b` becomes `a → a' → b' → b`. The two legs
 * (first and last segment) are left alone, and so is a segment ending inside the
 * card, which sidestepping cannot fix. Only inserts points, like `squareUp`.
 *
 * The corridor sits a leg and a HALF out, not one leg. One leg beyond a border is
 * exactly where every other edge landing on or leaving that side runs its
 * perpendicular leg, so a detour there drew over a neighbour's approach and the
 * two wires could no longer be told apart (#844 FP iter-5, finding 3).
 */
function sidestepCrossings(
  points: Point[],
  rect: AnchorRect,
  leg: number,
  far: boolean,
): Point[] {
  const lane = leg * 1.5;
  const corridor = (v: number, low: number, high: number) => {
    const near = nearerCorridor(v, low, high, lane);
    return far ? (near < low ? high + lane : low - lane) : near;
  };
  const strictlyInside = (p: Point) =>
    p.x > rect.x + TOL &&
    p.x < rect.x + rect.width - TOL &&
    p.y > rect.y + TOL &&
    p.y < rect.y + rect.height - TOL;
  const out: Point[] = [points[0]];
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const isLeg = i === 1 || i === points.length - 1;
    if (!isLeg && !strictlyInside(a) && !strictlyInside(b) && crossesRect([a, b], rect)) {
      if (Math.abs(a.x - b.x) < TOL) {
        const x = corridor(a.x, rect.x, rect.x + rect.width);
        out.push({ x, y: a.y }, { x, y: b.y });
      } else {
        const y = corridor(a.y, rect.y, rect.y + rect.height);
        out.push({ x: a.x, y }, { x: b.x, y });
      }
    }
    out.push(b);
  }
  return out;
}
