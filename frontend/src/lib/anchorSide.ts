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
  return type === "doc-only" || type === "code-mutating";
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
 * incoming arrow anchors on the side closest to where the user released — not
 * always the left. The decided rule: the side of the target card nearest the
 * drop point.
 *
 * The drop point may land inside the card or anywhere around it; we compare the
 * perpendicular distance to each of the four edges and pick the smallest. Ties
 * (equidistant edges) resolve in left, right, top, bottom order, so left wins
 * over right and the horizontal sides win over the vertical ones. (The overall
 * legacy left default is preserved by the caller, which persists a chosen side
 * only when it is NOT left — see `EditCanvas.onConnectEnd`.)
 */
export function chooseAnchorSide(
  drop: { x: number; y: number },
  rect: AnchorRect,
): PortSide {
  const left = Math.abs(drop.x - rect.x);
  const right = Math.abs(drop.x - (rect.x + rect.width));
  const top = Math.abs(drop.y - rect.y);
  const bottom = Math.abs(drop.y - (rect.y + rect.height));

  // Ties resolve in left, right, top, bottom order (strict `<`), keeping the
  // legacy left default stable for a centred drop.
  let best: PortSide = "left";
  let bestDist = left;
  if (right < bestDist) {
    best = "right";
    bestDist = right;
  }
  if (top < bestDist) {
    best = "top";
    bestDist = top;
  }
  if (bottom < bestDist) {
    best = "bottom";
  }
  return best;
}
