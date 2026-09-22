/**
 * Placement maths for the two kinds of edge label (#845).
 *
 * Both are pure functions of the drawn path, so `OrthogonalEdge` stays a
 * renderer and the placement rules (which are the reviewable part) are testable
 * without a canvas.
 *
 * This module is #845's own: the wiring-grid maths of #844 lands beside it in
 * `wiringGrid.ts`. Keeping the label rules here is what lets the two tickets be
 * implemented on the same integration branch without fighting over one file.
 */

import type { Point } from "./orthogonalRouter";

export type LabelAxis = "horizontal" | "vertical";

/** Orientation of the segment `a → b`, or null when the two points coincide. */
export function segmentAxis(a: Point, b: Point): LabelAxis | null {
  if (Math.abs(a.x - b.x) < 0.5 && Math.abs(a.y - b.y) < 0.5) return null;
  return Math.abs(b.x - a.x) >= Math.abs(b.y - a.y) ? "horizontal" : "vertical";
}

/**
 * Default placement of an output label around the base of the arrow (#845):
 * alternating sides, walking outwards — 1st left/above, 2nd right/below, 3rd
 * beyond the 1st, 4th beyond the 2nd. Which pair of sides is used depends on
 * whether the arrow *leaves* horizontally or vertically.
 *
 * `align` matters as much as the offset: a centred label straddles the
 * departure point, so anything longer than the gap disappears under the source
 * card. The label is anchored on its near edge instead and grows away from the
 * node, along the wire.
 *
 * `dir` is +1 when the first segment travels right (horizontal) or down
 * (vertical), -1 otherwise.
 */
export function outputLabelPlacement(
  index: number,
  firstAxis: LabelAxis,
  dir: 1 | -1,
): { x: number; y: number; align: "start" | "end" } {
  const GAP = 9; // clearance from the card's rim, along the wire
  const ALONG = 26; // how far down a vertical first segment the labels sit
  // 20 clears the condition pill, which is centred on the stroke and ~18 tall
  // with its border: half the pill plus half a label line, so the two never
  // graze on a short edge.
  const ACROSS = 20; // first rank's clearance from the stroke
  const RANK = 15; // each further rank steps out by this much
  const rank = Math.floor(index / 2);
  const across = (index % 2 === 0 ? -1 : 1) * (ACROSS + rank * RANK);
  return firstAxis === "horizontal"
    ? { x: dir * GAP, y: across, align: dir > 0 ? "start" : "end" }
    : { x: across, y: dir * ALONG, align: across < 0 ? "end" : "start" };
}

/**
 * Whether the output labels show by default: on iff the source node declares
 * **two or more** outputs (#845). A single-output node would just be repeating
 * itself on the canvas — the panel already names the port.
 */
export function outputLabelsDefault(declaredOutputCount: number): boolean {
  return declaredOutputCount >= 2;
}

/**
 * The effective toggle: the per-edge `show_output_labels` when the author set
 * one, the count-derived default otherwise (`null`/`undefined` ⇒ unset).
 */
export function resolveOutputLabels(
  showOutputLabels: boolean | null | undefined,
  declaredOutputCount: number,
): boolean {
  return showOutputLabels ?? outputLabelsDefault(declaredOutputCount);
}

/**
 * Where the condition pill sits by default: at the path's midpoint *along* the
 * wire (#176, unchanged) but nudged off the stroke, perpendicular to the
 * segment it sits on.
 *
 * Straddling the stroke is what the pre-#845 pill did, and on a straight edge
 * that puts it exactly on top of the segment drag handle — which is also at the
 * midpoint. The pill then eats the handle's pointer events and the segment
 * cannot be dragged at all. Nudging the pill clear keeps both gestures alive.
 */
export function conditionLabelOffset(segmentAxisAt: LabelAxis): Point {
  const CLEAR = 15;
  return segmentAxisAt === "horizontal" ? { x: 0, y: -CLEAR } : { x: -CLEAR, y: 0 };
}

/**
 * Axis and direction of the path's FIRST segment — what the alternating output
 * placement is read against. A degenerate path (one point, or a zero-length
 * first segment) reads as a rightward horizontal departure, which is the shape
 * every seeded edge has.
 */
export function firstSegmentHeading(points: Point[]): { axis: LabelAxis; dir: 1 | -1 } {
  const from = points[0];
  const to = points[1] ?? points[0];
  if (!from || !to) return { axis: "horizontal", dir: 1 };
  const axis = segmentAxis(from, to) ?? "horizontal";
  const delta = axis === "horizontal" ? to.x - from.x : to.y - from.y;
  return { axis, dir: delta < 0 ? -1 : 1 };
}

/** Orientation of the segment the path's midpoint falls on. */
export function midpointAxis(points: Point[], mid: Point): LabelAxis {
  for (let i = 0; i < points.length - 1; i++) {
    const a = points[i];
    const b = points[i + 1];
    const within =
      Math.min(a.x, b.x) - 0.5 <= mid.x &&
      mid.x <= Math.max(a.x, b.x) + 0.5 &&
      Math.min(a.y, b.y) - 0.5 <= mid.y &&
      mid.y <= Math.max(a.y, b.y) + 0.5;
    if (within) return segmentAxis(a, b) ?? "horizontal";
  }
  return "horizontal";
}
