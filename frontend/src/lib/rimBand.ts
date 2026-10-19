// The band of a node's border a wire is pulled from (CORE-88). Pure geometry, so
// the screen-constant width can be reasoned about — and tested — without a canvas.

import type { PortSide } from "../shared/generated/PortSide";

/** Width of the band ON SCREEN, whatever the zoom. */
export const RIM_SCREEN_PX = 8;
/** At most this much of the band, on screen, lies inside the card. */
export const RIM_INSIDE_MAX_PX = 3;
/** …and never more than this share of the card's own depth on that side, so a
 *  dezoomed card keeps a middle to grab and move it by. */
export const RIM_INSIDE_MAX_SHARE = 0.25;

/** The band on one side, in FLOW px (the card's own space): its full width, and
 *  how much of it lies inside the card — the rest lies outside. */
export interface RimBand {
  width: number;
  inside: number;
  outside: number;
}

/**
 * The band at `zoom` for a card of `size` (flow px). `depth` is the card's
 * extent ACROSS the side — its height for top/bottom, its width for left/right.
 * Constant on screen: 8px, at most 3px (and 25 % of the depth) inside.
 */
export function rimBand(zoom: number, depth: number): RimBand {
  const z = zoom > 0 ? zoom : 1;
  const width = RIM_SCREEN_PX / z;
  const inside = Math.min(RIM_INSIDE_MAX_PX / z, Math.max(depth, 0) * RIM_INSIDE_MAX_SHARE);
  return { width, inside, outside: width - inside };
}

/** The card's extent across `side`. */
export function rimDepth(side: PortSide, size: { width: number; height: number }): number {
  return side === "top" || side === "bottom" ? size.height : size.width;
}
