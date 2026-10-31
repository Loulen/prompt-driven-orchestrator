// The AUTO route of an edge as the canvas draws it: the router's path, snapped
// onto the canvas lattice, its two legs squared to the anchored sides (#844).

import { routeOrthogonal, type Point, type Rect } from "./orthogonalRouter";
import { snapPolyline } from "./wiringGrid";
import { enforcePerpendicularEnds, landingLeg } from "./anchorLanding";
import { dedupeCollinear } from "./polylineCleanup";
import type { PortSide } from "../shared/generated/PortSide";

/**
 * The lattice an AUTO route snaps to: the canvas's own, through the origin —
 * deliberately NOT the wiring session's current origin.
 *
 * A gesture re-origins the lattice on its own tip when Shift is released
 * (ADR-0072), which is right for the wire being drawn and wrong for every other
 * edge on the canvas: reading the session origin here made every auto route on
 * screen jump a few pixels the moment someone started drawing a wire somewhere
 * else. An auto route belongs to no gesture, so it snaps to the shared lattice
 * and stays put.
 */
const CANVAS_LATTICE: Point = { x: 0, y: 0 };

export interface AutoEdgeRouteInput {
  source: Point;
  target: Point;
  sourceSide: PortSide;
  targetSide: PortSide;
  sourceRect?: Rect;
  targetRect?: Rect;
  /** Every other card on the canvas, and the loop regions (`region`). */
  obstacles: (Rect & { region?: boolean })[];
  /** The wiring grid's step: it sets the legs and the snap. */
  step: number;
}

/** True when an axis-aligned segment enters `rect` or runs along its border. */
function segmentEnters(a: Point, b: Point, rect: Rect): boolean {
  const HUG = 2;
  return (
    Math.max(a.x, b.x) > rect.x - HUG &&
    Math.min(a.x, b.x) < rect.x + rect.width + HUG &&
    Math.max(a.y, b.y) > rect.y - HUG &&
    Math.min(a.y, b.y) < rect.y + rect.height + HUG
  );
}

/** Inside `rect` or on its border. */
function holds(rect: Rect, p: Point): boolean {
  return p.x >= rect.x && p.x <= rect.x + rect.width && p.y >= rect.y && p.y <= rect.y + rect.height;
}

/** True when the route cuts a card, or hugs its border: any other card, its
 *  source card past the departure leg, its target card before the landing leg.
 *  A loop region is no card: the wires of its members run inside it. */
function cutsACard(points: Point[], input: AutoEdgeRouteInput): boolean {
  const cuts = (pts: Point[], rect: Rect) => pts.some((p, i) => i > 0 && segmentEnters(pts[i - 1], p, rect));
  if (input.obstacles.some((o) => !o.region && cuts(points, o))) return true;
  if (input.sourceRect && cuts(points.slice(1), input.sourceRect)) return true;
  return input.targetRect != null && cuts(points.slice(0, -1), input.targetRect);
}

/**
 * « Re-route automatically » produces a grid-ALIGNED auto path (#844): the
 * router's bends are snapped onto the wiring lattice so an auto edge and a
 * hand-drawn one line up instead of missing by a few pixels. It lands
 * perpendicular too, so the reset does not undo the arrowhead's orientation.
 *
 * Snapping moves a bend by up to half a step, which can push a segment that
 * cleared a card onto its border or into it: such a route falls back to the
 * router's own bends.
 */
export function autoEdgeRoute(input: AutoEdgeRouteInput): Point[] {
  const { source, target, sourceSide, targetSide, sourceRect, targetRect, step } = input;
  const leg = landingLeg(step);
  // A loop region holding one end of the wire is the room it leaves or enters,
  // not an obstacle: as one, it boxed the endpoint in, the router found no way
  // out and the fallback ran the wire through the next card down.
  const obstacles = input.obstacles.filter((o) => !o.region || !(holds(o, source) || holds(o, target)));
  const auto = routeOrthogonal({
    source,
    target,
    obstacles,
    targetSide,
    sourceSide,
    sourceRect,
    targetRect,
    // Half a cell past the leg, so the lattice snap below never rounds the
    // router's first bend back inside the source leg.
    sourceStub: leg + step / 2,
  });
  // `manual: false`: the router's bends are not the user's pins, so a wire that
  // has to go around its target is re-laid from its last real corner, and one
  // that runs back through its own source card goes around it.
  const enforce = (points: Point[]) =>
    enforcePerpendicularEnds(points, sourceSide, targetSide, leg, targetRect, { manual: false, sourceRect });
  const snapped = enforce(snapPolyline(auto, CANVAS_LATTICE, step));
  if (!cutsACard(snapped, input)) return withoutLegSpurs(snapped, input);
  const unsnapped = enforce(auto);
  return cutsACard(unsnapped, input) ? snapped : withoutLegSpurs(unsnapped, input);
}

/** `a → b → c` on one line, `c` back towards `a`. */
function reverses(a: Point, b: Point, c: Point): boolean {
  if (a.y === b.y && b.y === c.y) return (b.x - a.x) * (c.x - b.x) < 0;
  if (a.x === b.x && b.x === c.x) return (b.y - a.y) * (c.y - b.y) < 0;
  return false;
}

/**
 * Folds the spur a snapped bend leaves beside a leg. The router's lead-in and
 * the enforced leg differ by a few pixels once rounded onto the lattice, so the
 * wire overshot the approach point and came back to it: `… 320 → 310 → 330`
 * into a left side, a stub drawn over the leg. The run before the overshoot is
 * slid onto the approach point instead, and the leg keeps its full length. A
 * slide that would cut a card keeps the route as it was.
 */
function withoutLegSpurs(points: Point[], input: AutoEdgeRouteInput): Point[] {
  let out = points;
  const n = out.length;
  // The run before the overshoot is perpendicular to the leg: it slides along it.
  const onto = (p: Point, approach: Point, legHorizontal: boolean): Point =>
    legHorizontal ? { x: approach.x, y: p.y } : { x: p.x, y: approach.y };
  if (n >= 5 && reverses(out[n - 3], out[n - 2], out[n - 1])) {
    const approach = out[n - 2];
    out = [...out.slice(0, n - 4), onto(out[n - 4], approach, approach.y === out[n - 1].y), ...out.slice(n - 2)];
  }
  if (out.length >= 5 && reverses(out[0], out[1], out[2])) {
    const approach = out[1];
    out = [out[0], approach, onto(out[3], approach, approach.y === out[0].y), ...out.slice(4)];
  }
  if (out === points) return points;
  const tidy = [out[0], ...dedupeCollinear(out.slice(1, -1)), out[out.length - 1]];
  return cutsACard(tidy, input) ? points : tidy;
}
