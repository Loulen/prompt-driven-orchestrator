// The live wiring preview (#844). No longer a single orthogonal L recomputed from
// the cursor: a trace that GROWS on the wiring grid, one segment per crossed cell.
// Shift frees the pending segment, and releasing Shift re-origins the grid on the
// current point (ADR-0072).
//
// The gesture's own logic lives in `lib/wiringGesture.ts`; this component only
// feeds it pointer and key events, publishes the drawn polyline for
// `onConnectEnd`, and draws.

import { useCallback, useEffect, useRef, useState } from "react";
import {
  useConnection,
  useReactFlow,
  useStoreApi,
  type ConnectionLineComponentProps,
} from "@xyflow/react";
import { pathToSvg } from "../lib/edgePath";
import type { Point } from "../lib/orthogonalRouter";
import type { PortSide } from "../types";
import { sideFromRimHandle } from "../lib/anchorSide";
import { useWiringGridStep } from "../hooks/useWiringGrid";
import {
  advanceGesture,
  gesturePath,
  hoverLanding,
  startGesture,
  type LandingTarget,
  type WiringGesture,
} from "../lib/wiringGesture";
import { WIRE } from "../lib/wiringColors";
import { pendingSource, wiringGesture, wiringTrace } from "../lib/wiringSession";
import { useWiringStore } from "../stores/wiringStore";

export default function DragConnectionLine({
  fromX,
  fromY,
  toX,
  toY,
}: ConnectionLineComponentProps) {
  const connection = useConnection();
  const store = useStoreApi();
  const setOrigin = useWiringStore((s) => s.setOrigin);
  const { screenToFlowPosition } = useReactFlow();
  // The step in effect when the gesture starts (#877). A size change mid-drag is
  // not a gesture anyone performs; the lattice of a trace never changes under it.
  const step = useWiringGridStep();

  const pressedSide = sideFromRimHandle(connection.fromHandle?.id);

  // The gesture is STATE, advanced only from event handlers. Advancing it while
  // rendering would double-step every pointer move under `StrictMode`.
  const [gesture, setGesture] = useState<WiringGesture>(() => {
    // `pendingSource` is where the rim was actually pressed; xyflow's `fromX/fromY`
    // are the handle's CENTRE, and the whole point of the rim drag-source is that
    // a wire does not start there.
    const from = pendingSource.point ?? { x: fromX, y: fromY };
    return startGesture(from, pendingSource.anchor?.side ?? pressedSide, step);
  });
  // A mirror read ONLY from handlers, so a pointer move can advance from the
  // freshest gesture without waiting for a re-render.
  const latest = useRef(gesture);

  // The lattice the overlay draws is the one this gesture is snapping to.
  useEffect(() => {
    setOrigin(gesture.origin);
  }, [gesture.origin, setOrigin]);

  const step_ = step;
  const frame = useCallback(
    (cursor: Point, shift: boolean) => {
      const state = store.getState();
      const live = state.connection.inProgress ? state.connection : null;
      const landing = hoverLanding(
        cursor,
        live?.toNode ?? null,
        live?.toHandle ?? null,
        live?.fromNode?.id ?? null,
      );
      const next = advanceGesture(latest.current, {
        cursor,
        shift,
        overTarget: landing != null,
        step: step_,
      });
      latest.current = next;
      // Published synchronously: `onConnectEnd` fires on the very next event and
      // reads this to persist the drawn route.
      wiringTrace.points = gesturePath(next, landing, step_);
      wiringGesture.current = next;
      setGesture(next);
    },
    [store, step_],
  );

  // THE REAL CURSOR. xyflow's `toX`/`toY` are not it: as soon as the pointer is
  // over a connectable handle, xyflow snaps them onto that handle's position —
  // which for our body/anchor handles is the card's side CENTRE. That snap is what
  // makes the preview jump to the middle of the node. The raw pointer is tracked
  // here and converted to flow space, so the landing anchor is computed from where
  // the cursor actually is.
  useEffect(() => {
    const move = (e: PointerEvent) => {
      frame(screenToFlowPosition({ x: e.clientX, y: e.clientY }), e.shiftKey);
    };
    // Shift is tracked on the window: the pointer is captured by the drag, so the
    // key events never reach the SVG. A release with the mouse still re-origins
    // at once, without waiting for the next move.
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Shift") return;
      frame(latest.current.cursor, e.type === "keydown");
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("keydown", key);
    window.addEventListener("keyup", key);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("keydown", key);
      window.removeEventListener("keyup", key);
    };
  }, [frame, screenToFlowPosition]);

  // Landing preview: while the cursor is over a candidate target, the wire must
  // end where it will actually be pinned — the anchor `onConnectEnd` is about to
  // persist (nearest side, per-pixel offset) — and not at the node's centre, which
  // is where the handle's own geometry would drag it.
  const live = connection.inProgress ? connection : null;
  const landing: LandingTarget | null = hoverLanding(
    gesture.cursor,
    live?.toNode ?? null,
    live?.toHandle ?? null,
    live?.fromNode?.id ?? null,
  );
  const points = gesturePath(gesture, landing, step);
  const tip = points[points.length - 1] ?? { x: toX, y: toY };

  return (
    <g data-testid="drag-connection-line">
      <path d={pathToSvg(points)} fill="none" stroke={WIRE} strokeWidth={1.5} />
      {/* Departure point and every committed bend. */}
      {points.slice(0, -1).map((p, i) => (
        <circle key={i} cx={p.x} cy={p.y} r={i === 0 ? 2.5 : 1.8} fill={WIRE} opacity={0.9} />
      ))}
      {/* Over a target: an arrowhead on the border, at the exact spot the wire will
          be pinned. Otherwise: the snap indicator — a ring on the cell the tip has
          locked onto, hollow and dashed while Shift frees it, so the two modes are
          told apart at a glance. That indicator IS the Shift feedback; no cursor
          badge chases the pointer. */}
      {landing ? (
        <polygon
          points={arrowHead(landing.point, landing.anchor.side)}
          fill={WIRE}
          data-testid="wiring-landing-arrow"
        />
      ) : (
        <circle
          cx={tip.x}
          cy={tip.y}
          r={gesture.shift ? 4 : 3.2}
          fill={gesture.shift ? "none" : WIRE}
          stroke={WIRE}
          strokeWidth={gesture.shift ? 1.2 : 0}
          strokeDasharray={gesture.shift ? "2 2" : undefined}
          data-testid="wiring-snap-indicator"
          data-snapped={gesture.shift ? "false" : "true"}
        />
      )}
    </g>
  );
}

/** A small triangle pointing INTO `side`, tipped on the border point. */
function arrowHead(p: Point, side: PortSide): string {
  const L = 8; // length, along the arrival direction
  const W = 4.5; // half-width, across it
  switch (side) {
    case "left":
      return `${p.x},${p.y} ${p.x - L},${p.y - W} ${p.x - L},${p.y + W}`;
    case "right":
      return `${p.x},${p.y} ${p.x + L},${p.y - W} ${p.x + L},${p.y + W}`;
    case "top":
      return `${p.x},${p.y} ${p.x - W},${p.y - L} ${p.x + W},${p.y - L}`;
    case "bottom":
      return `${p.x},${p.y} ${p.x - W},${p.y + L} ${p.x + W},${p.y + L}`;
  }
}
