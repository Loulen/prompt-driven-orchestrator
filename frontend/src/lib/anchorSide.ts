import type { NodeType, PortSide } from "../types";

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
 * The work-node types (`doc-only`, `code-mutating`) are emergent. This is keyed
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
  return type === "doc-only" || type === "code-mutating" || type === "script";
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
