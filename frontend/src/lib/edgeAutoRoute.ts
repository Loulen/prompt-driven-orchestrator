// The AUTO route of an edge as the canvas draws it: the router's path, snapped
// onto the canvas lattice, its two legs squared to the anchored sides (#844).

import { routeOrthogonal, type Point, type Rect } from "./orthogonalRouter";
import { snapPolyline } from "./wiringGrid";
import { enforcePerpendicularEnds, landingLeg } from "./anchorLanding";
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
  /** Every other card on the canvas. */
  obstacles: Rect[];
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

/** True when the route cuts a card, or hugs its border: any other card, its
 *  source card past the departure leg, its target card before the landing leg. */
function cutsACard(points: Point[], input: AutoEdgeRouteInput): boolean {
  const cuts = (pts: Point[], rect: Rect) => pts.some((p, i) => i > 0 && segmentEnters(pts[i - 1], p, rect));
  if (input.obstacles.some((o) => cuts(points, o))) return true;
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
  const { source, target, sourceSide, targetSide, sourceRect, targetRect, obstacles, step } = input;
  const leg = landingLeg(step);
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
  if (!cutsACard(snapped, input)) return snapped;
  const unsnapped = enforce(auto);
  return cutsACard(unsnapped, input) ? snapped : unsnapped;
}
