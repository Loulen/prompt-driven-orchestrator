/**
 * Where the tour popover sits relative to the Projecteur's hole (#823) — pure
 * geometry, so the flipping rules can be checked without a browser.
 *
 * The design pins the card to the hole (never to the bottom of the screen): the
 * instruction and the thing to click must be readable in one glance. It therefore
 * has to flip when the hole is near an edge, and it must never leave the viewport —
 * an instruction the user has to scroll to is worse than no instruction.
 */

export interface Box {
  top: number;
  left: number;
  width: number;
  height: number;
}

export type PopoverSide = "right" | "left" | "bottom" | "top";

export interface Placement {
  top: number;
  left: number;
  /** Which side of the hole the card landed on — the arrow points back at it. */
  side: PopoverSide;
}

/** Distance between the hole's edge and the card. */
const GAP = 12;
/** Never let the card touch the window edge. */
const MARGIN = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Pick a side with room for the whole card, preferring right → left → bottom →
 * top, then clamp the cross axis into the viewport.
 *
 * With no hole (the target is gone and the tour is about to say so) the card
 * centres itself, which is also where the failure card wants to be.
 */
export function placePopover(
  hole: Box | null,
  card: { width: number; height: number },
  viewport: { width: number; height: number },
): Placement {
  if (!hole) {
    return {
      top: Math.max(MARGIN, (viewport.height - card.height) / 2),
      left: Math.max(MARGIN, (viewport.width - card.width) / 2),
      side: "bottom",
    };
  }

  const right = hole.left + hole.width;
  const bottom = hole.top + hole.height;
  const fitsRight = right + GAP + card.width + MARGIN <= viewport.width;
  const fitsLeft = hole.left - GAP - card.width - MARGIN >= 0;
  const fitsBottom = bottom + GAP + card.height + MARGIN <= viewport.height;

  const side: PopoverSide = fitsRight ? "right" : fitsLeft ? "left" : fitsBottom ? "bottom" : "top";

  if (side === "right" || side === "left") {
    // A target can be scrolled half out of the window; the instruction must not
    // follow it out, so BOTH axes are clamped even on the axis the side pins.
    const anchored = side === "right" ? right + GAP : hole.left - GAP - card.width;
    // Centre on the hole vertically, then clamp — a tall target keeps the card
    // beside its middle rather than sliding off the bottom of the window.
    const wanted = hole.top + hole.height / 2 - card.height / 2;
    return {
      left: clamp(anchored, MARGIN, Math.max(MARGIN, viewport.width - card.width - MARGIN)),
      top: clamp(wanted, MARGIN, Math.max(MARGIN, viewport.height - card.height - MARGIN)),
      side,
    };
  }

  const top = side === "bottom" ? bottom + GAP : hole.top - GAP - card.height;
  const wanted = hole.left + hole.width / 2 - card.width / 2;
  return {
    top: clamp(top, MARGIN, Math.max(MARGIN, viewport.height - card.height - MARGIN)),
    left: clamp(wanted, MARGIN, Math.max(MARGIN, viewport.width - card.width - MARGIN)),
    side,
  };
}
