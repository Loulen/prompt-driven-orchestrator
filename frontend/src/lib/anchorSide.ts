import type { EdgeAnchor } from "../shared/generated/EdgeAnchor";
import type { NodeType } from "../shared/generated/NodeType";
import type { PortSide } from "../shared/generated/PortSide";
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
 * `start` has no inputs; `end` keeps its declared `result`. Where an incoming wire LANDS is a separate question —
 * see {@link landsByDrop}: End lands by drop like a work node (#840).
 */
export function isEmergentInputNode(type: NodeType): boolean {
  // #248: a `script` node consumes whole artifacts by edge like a work node, so
  // its inputs are emergent too — anchor incoming edges to the body by drop.
  return type === "agent" || type === "script";
}

/**
 * Whether an incoming edge lands on this node where it was DROPPED (any border,
 * per-pixel `target_anchor`), rather than being pinned to a declared handle.
 *
 * Every emergent work node does, and so does the End marker (#840): its declared
 * `result` is the pipeline's contract — the edge still carries that port — but
 * it is not a place on the card. Pinning the arrow to `result`'s declared side
 * (`left` in every real pipeline) made End the one target that refused a wire on
 * its top, right or bottom border. `start` takes no incoming edge.
 *
 * One question, asked by the card's handles, the edge derivation, the landing
 * preview and the drop — so the four cannot disagree on where a wire lands.
 */
export function landsByDrop(type: NodeType): boolean {
  return isEmergentInputNode(type) || type === "end";
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
 * position (#168). Only a body anchor handle does (a work node's, or End's since
 * #840); a declared port handle keeps its fixed declared
 * side and must be left untouched (AC: declared ports unaffected).
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
 * Where a declared handle pins an edge: the middle of the handle's own `side`,
 * from the handle's CENTRE and size.
 *
 * This is xyflow's `getHandlePosition` rule, restated — the renderer is handed
 * exactly this point as `targetX/targetY`, so a preview that lands anywhere else
 * draws a wire the edge will not keep. It matters for any target that pins the
 * wire to a declared handle (End lands by drop since #840).
 */
export function handlePin(
  centre: Point,
  size: { width: number; height: number },
  side: PortSide,
): Point {
  switch (side) {
    case "top":
      return { x: centre.x, y: centre.y - size.height / 2 };
    case "bottom":
      return { x: centre.x, y: centre.y + size.height / 2 };
    case "left":
      return { x: centre.x - size.width / 2, y: centre.y };
    case "right":
      return { x: centre.x + size.width / 2, y: centre.y };
  }
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
