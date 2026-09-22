// The wiring gesture as a pure state machine (#844).
//
// One frame of drawing an edge is: where the cursor is, whether Shift is down,
// and whether the pointer has entered a candidate target. Everything the gesture
// accumulates — the committed trace, the lattice origin — follows from those by
// the reducer below, so the connection-line component only has to feed it
// pointer events and draw what comes back.
//
// Keeping it here rather than inside the component is not only testability. The
// app renders under `StrictMode`, which invokes a component's body twice: a trace
// advanced as a side effect of rendering would walk two cells per pointer move,
// visibly and only in development.

import type { EdgeAnchor, PortSide } from "../types";
import type { Point } from "./orthogonalRouter";
import {
  anchorFromPoint,
  anchorPoint,
  approachPoint,
  clipOutside,
  landingConnector,
  landingLeg,
  dropAnchor,
  type AnchorRect,
} from "./anchorSide";
import {
  advanceTrace,
  freeContinuation,
  reOrigin,
  snapToGrid,
  type Axis,
  type Trace,
} from "./wiringGrid";

/** Where the wire would be pinned if the pointer were released now. */
export interface LandingTarget {
  rect: AnchorRect;
  anchor: EdgeAnchor;
  point: Point;
}

export interface WiringGesture {
  /** Committed bends. Grows one segment per crossed grid cell. */
  trace: Trace;
  /** The point the lattice currently passes through (ADR-0072). */
  origin: Point;
  /** The raw cursor, in flow coordinates. */
  cursor: Point;
  /** Shift held right now — the pending segment is off the grid. */
  shift: boolean;
  /** Shift was held on the previous frame, so the next grid frame must first
   *  commit the freehand run and re-origin on its tip. */
  wasShift: boolean;
}

export const AXIS_FOR_SIDE: Record<PortSide, Axis> = {
  left: "horizontal",
  right: "horizontal",
  top: "vertical",
  bottom: "vertical",
};

/**
 * The gesture at the moment of the press.
 *
 * The trace is SEEDED with the perpendicular leg out of the pressed side, not
 * just with the departure point: a heading names the axis of the first segment,
 * never its direction, so a cursor that starts back over the card would otherwise
 * draw a first segment running into the card it just left.
 */
export function startGesture(from: Point, side: PortSide | null, step: number): WiringGesture {
  const seeded = side ? [from, approachPoint(from, side, landingLeg(step))] : [from];
  return {
    trace: { points: seeded, heading: side ? AXIS_FOR_SIDE[side] : "horizontal" },
    origin: reOrigin(from),
    cursor: from,
    shift: false,
    wasShift: false,
  };
}

/**
 * One frame. Advances the trace by at most one bend, or freezes it.
 *
 * Three regimes, and the frozen ones commit NOTHING — that is what makes Shift
 * and the landing reversible by simply moving the mouse back:
 *
 *   - over a target: frozen, because the landing anchor is per-pixel by design
 *     and a snapped final approach cannot reach it;
 *   - Shift down: frozen, the pending L follows the cursor off-grid;
 *   - otherwise: the cursor is snapped onto the lattice and the trace advances —
 *     and if Shift has JUST been released, the freehand run is first committed
 *     verbatim and the lattice re-originated on its tip, so the cells that follow
 *     line up with what was drawn rather than with where the gesture began.
 */
export function advanceGesture(
  gesture: WiringGesture,
  input: { cursor: Point; shift: boolean; overTarget: boolean; step: number },
): WiringGesture {
  const { cursor, shift, overTarget, step } = input;
  if (overTarget) return { ...gesture, cursor };
  if (shift) return { ...gesture, cursor, shift: true, wasShift: true };

  let trace = gesture.trace;
  let origin = gesture.origin;
  if (gesture.wasShift) {
    const freed = freeContinuation(trace, cursor);
    trace = { points: freed, heading: trace.heading };
    origin = reOrigin(freed[freed.length - 1]);
  }
  return {
    trace: advanceTrace(trace, snapToGrid(cursor, origin, step)),
    origin,
    cursor,
    shift: false,
    wasShift: false,
  };
}

/**
 * The polyline to draw for this frame.
 *
 * Over a target, the trace is CLIPPED first: it keeps growing on the grid right
 * up to the moment xyflow reports the pointer as being over a node, by which time
 * the tip can already be on the border — and building the landing connector from
 * there makes the wire double back out of the card and in again, a visible spur
 * with the arrowhead behind it.
 */
export function gesturePath(
  gesture: WiringGesture,
  landing: LandingTarget | null,
  step: number,
): Point[] {
  if (landing) {
    const leg = landingLeg(step);
    const committed = clipOutside(gesture.trace.points, landing.rect, leg);
    return [
      ...committed.slice(0, -1),
      ...landingConnector(
        committed[committed.length - 1],
        landing.point,
        landing.anchor.side,
        leg,
        landing.rect,
      ),
    ];
  }
  if (gesture.shift) return freeContinuation(gesture.trace, gesture.cursor);
  return gesture.trace.points;
}

/**
 * The landing the cursor is currently aiming at on `rect`. The SAME rule the drop
 * will use, so the previewed arrowhead is where the wire gets pinned.
 *
 * `pinned` is the escape hatch for a target that does NOT anchor by drop position:
 * the End marker's declared `result`, a merge's `branches`. Those keep their own
 * fixed handle, so the wire lands on it whatever the cursor aimed at — and the
 * preview has to say so. Previewing a drop-chosen side there drew a landing the
 * edge would never render, and the points of that phantom approach were persisted
 * as waypoints inside the card (#844, FP finding 2).
 */
export function landingAt(
  cursor: Point,
  rect: AnchorRect,
  pinned?: { side: PortSide; point: Point } | null,
): LandingTarget {
  if (pinned) {
    // The offset merely DESCRIBES the pin (nothing persists it for such a
    // target); `point` is the pin itself, taken from the handle, never clamped.
    return {
      rect,
      anchor: anchorFromPoint(pinned.point, rect, pinned.side, 0),
      point: pinned.point,
    };
  }
  const anchor = dropAnchor(cursor, rect);
  return { rect, anchor, point: anchorPoint(rect, anchor, anchor.side) };
}
