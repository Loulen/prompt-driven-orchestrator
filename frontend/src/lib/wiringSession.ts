// The gesture in flight, between `onConnectStart` and `onConnectEnd` (#844).
//
// WHY A MODULE BOX AND NOT STATE. xyflow mounts the connection-line component
// itself, around the gesture, and hands it only its own props — there is no way
// to pass it the press position, and no way for it to hand the trace it drew back
// to `onConnectEnd`. Both facts are needed at the drop:
//
//   - the press position, because `onConnectStart(event, …)` is the ONLY place it
//     exists (xyflow gives the connection line the bound handle's CENTRE, and the
//     whole point of the rim drag-source is that a wire does not start there);
//   - the drawn trace, because the edge is born `mode: manual` carrying exactly
//     the waypoints the user drew.
//
// A React state or a store would re-render the whole canvas on every pointer
// move, for values nothing renders from. This is a per-gesture scratchpad, reset
// at each `onConnectStart`, consumed at `onConnectEnd`.

import type { EdgeAnchor } from "../types";
import type { Point } from "./orthogonalRouter";
import type { WiringGesture } from "./wiringGesture";

/** The polyline the connection line last drew, in flow coordinates. */
export const wiringTrace: { points: Point[] } = { points: [] };

/** The gesture state the connection line last reached, so the drop can rebuild
 *  its landing from xyflow's final connection state (see `droppedPath`). */
export const wiringGesture: { current: WiringGesture | null } = { current: null };

/** Where on the rim the gesture started: the flow point, and the anchor it
 *  resolves to on the source card. */
export const pendingSource: { point: Point | null; anchor: EdgeAnchor | null } = {
  point: null,
  anchor: null,
};

/** Forgets the gesture. Called when one starts and when one ends, so a cancelled
 *  drag never leaks its trace into the next edge. */
export function resetWiringSession(): void {
  wiringTrace.points = [];
  wiringGesture.current = null;
  pendingSource.point = null;
  pendingSource.anchor = null;
}
