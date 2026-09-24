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

import type { EdgeAnchor, NodeType, PortSide } from "../types";
import type { Point } from "./orthogonalRouter";
import {
  anchorFromPoint,
  anchorPoint,
  approachPoint,
  clipOutside,
  landingConnector,
  landingLeg,
  dropAnchor,
  handlePin,
  landsByDrop,
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
    return foldJunction(
      [
        ...committed.slice(0, -1),
        ...landingConnector(
          committed[committed.length - 1],
          landing.point,
          landing.anchor.side,
          leg,
          landing.rect,
        ),
      ],
      committed.length - 1,
    );
  }
  if (gesture.shift) return freeContinuation(gesture.trace, gesture.cursor);
  return gesture.trace.points;
}

/**
 * Drops the point where the committed trace hands over to the landing connector
 * while it sits mid-run — typically the last grid cell was traced AWAY from the
 * landing, and the connector ran straight back over it: a 40px spur in the
 * preview that the persisted route (collinear-merged) never had (#844 FP iter-2).
 * Only that junction is touched, and never the approach point or the anchor, so
 * the perpendicular leg is intact.
 */
function foldJunction(points: Point[], junction: number): Point[] {
  const out = points.map((p) => ({ ...p }));
  let j = junction;
  while (j >= 1 && j < out.length - 2) {
    const [a, b, c] = [out[j - 1], out[j], out[j + 1]];
    const collinear =
      (Math.abs(a.y - b.y) < 1e-6 && Math.abs(b.y - c.y) < 1e-6) ||
      (Math.abs(a.x - b.x) < 1e-6 && Math.abs(b.x - c.x) < 1e-6);
    if (!collinear) break;
    out.splice(j, 1);
    j -= 1;
  }
  return out;
}

/**
 * The landing the cursor is currently aiming at on `rect`. The SAME rule the drop
 * will use, so the previewed arrowhead is where the wire gets pinned.
 *
 * `pinned` is the escape hatch for a target that does NOT anchor by drop position:
 * a merge's `branches`. It keeps its own fixed handle, so the wire lands on it
 * whatever the cursor aimed at — and the preview has to say so. Previewing a drop-chosen side there drew a landing the
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

/** The slice of an xyflow internal node the landing needs. */
export interface HoveredNode {
  id: string;
  measured?: { width?: number; height?: number };
  internals: { positionAbsolute: Point };
  data?: Record<string, unknown>;
}

/** The slice of an xyflow handle the landing needs: its absolute CENTRE, its
 *  size, and its `Position` (whose values are the four side names). */
export interface HoveredHandle {
  x: number;
  y: number;
  width: number;
  height: number;
  position: string;
}

const SIDES: readonly PortSide[] = ["left", "right", "top", "bottom"];

/** The rect of the hovered node, or null when it is the gesture's own source or
 *  is not measured yet. */
export function hoveredRect(toNode: HoveredNode | null, fromNodeId: string | null): AnchorRect | null {
  if (!toNode || toNode.id === fromNodeId) return null;
  const width = toNode.measured?.width ?? 0;
  const height = toNode.measured?.height ?? 0;
  if (!width || !height) return null;
  return { x: toNode.internals.positionAbsolute.x, y: toNode.internals.positionAbsolute.y, width, height };
}

/**
 * Where the wire will be pinned on a target that does NOT anchor by drop (a
 * merge's `branches`): on that handle, on its own side, wherever the cursor
 * happens to be. `null` for a target that lands where it is dropped — an
 * emergent body, or the End marker (#840, see `landsByDrop`).
 *
 * Read off the live handle rather than assumed, because the handle is the thing
 * the renderer will use: xyflow's `toHandle` carries the handle's absolute CENTRE
 * plus its size and `position`, which is all `getHandlePosition` needs. Rounded
 * like `OrthogonalEdge` rounds its endpoints, so preview and edge agree to the
 * pixel.
 */
export function pinnedLanding(
  toNode: HoveredNode | null,
  toHandle: HoveredHandle | null,
  rect: AnchorRect,
): { side: PortSide; point: Point } | null {
  const nodeType = toNode?.data?.nodeType;
  if (typeof nodeType === "string" && landsByDrop(nodeType as NodeType)) return null;
  const side =
    toHandle && (SIDES as readonly string[]).includes(toHandle.position)
      ? (toHandle.position as PortSide)
      : "left";
  const pin = toHandle
    ? handlePin({ x: toHandle.x, y: toHandle.y }, toHandle, side)
    : anchorPoint(rect, null, side);
  return { side, point: { x: Math.round(pin.x), y: Math.round(pin.y) } };
}

/**
 * The landing for a cursor hovering `toNode` — or null when nothing is hovered.
 * One function for the preview and for the drop, so the two cannot drift.
 */
export function hoverLanding(
  cursor: Point,
  toNode: HoveredNode | null,
  toHandle: HoveredHandle | null,
  fromNodeId: string | null,
): LandingTarget | null {
  const rect = hoveredRect(toNode, fromNodeId);
  return rect ? landingAt(cursor, rect, pinnedLanding(toNode, toHandle, rect)) : null;
}

/**
 * The route a drop persists, rebuilt from the FINAL connection state.
 *
 * Not the polyline the last pointer move published. xyflow refreshes its hover
 * state (`toNode`/`toHandle`) on `mousemove`, which the browser dispatches AFTER
 * the `pointermove` the gesture listens to — so each frame reads the hover of the
 * previous one. When the pointer enters the target and is released on the very
 * next event, the last published trace has no landing at all: the saved waypoints
 * stop short, and the renderer squares the gap straight through the card, while
 * the (fresher) preview had gone around it (#844, FP iter-2). The drop's own
 * `toNode`/`toHandle` and release point are current, so the landing is recomputed
 * from them.
 */
export function droppedPath(
  gesture: WiringGesture,
  drop: Point,
  toNode: HoveredNode | null,
  toHandle: HoveredHandle | null,
  fromNodeId: string | null,
  step: number,
): Point[] {
  const landing = hoverLanding(drop, toNode, toHandle, fromNodeId);
  // The gesture may not even know the pointer is over the target yet (see
  // above); advancing it with `overTarget` only records the release point.
  const next = advanceGesture(gesture, { cursor: drop, shift: gesture.shift, overTarget: landing != null, step });
  return gesturePath(next, landing, step);
}
