/**
 * Where a newly created node lands on the canvas — pure geometry, so the rule
 * can be checked without a canvas.
 *
 * The rule is « don't drop it on top of something ». It used to be enforced with
 * a 30-pixel threshold and a 40-pixel nudge, which is not the same rule: a card
 * is roughly 180 × 80, so two cards 40 pixels apart pass that test while sitting
 * almost entirely on each other. The visible cost was an edge you cannot click —
 * *First pipeline* has the reader create an implementer and a tester, then asks
 * them to select the edge between the two, and that edge ran underneath the very
 * cards the tour had just had them make (found by the FP of #825).
 *
 * So the free-spot test is the card's own footprint, and the search walks a small
 * grid around the drop point — right first (a pipeline reads left to right), then
 * down a row — rather than sliding diagonally into the distance.
 */

export interface Point {
  x: number;
  y: number;
}

/** Approximate default node-card footprint; nodes auto-size around this. */
export const CARD_WIDTH = 180;
export const CARD_HEIGHT = 80;

/** Breathing room between two cards, so « not overlapping » also reads as such. */
const GAP = 40;

const STEP_X = CARD_WIDTH + GAP;
const STEP_Y = CARD_HEIGHT + GAP;
/** How wide a row of candidates gets before the search drops to the next one. */
const COLUMNS = 4;
/** Candidates tried before giving up and using the last one. A canvas dense
 *  enough to fill a 4 × 5 grid around the drop point has no free spot to find;
 *  landing somewhere plausible beats searching forever. */
const CANDIDATES = 20;

/**
 * Where the canvas lays out a node the document gives no position for — a fresh
 * pipeline's Start and End, which are written without a `view`. A column, in
 * document order.
 *
 * Shared with the drop search on purpose: a node whose place the canvas chose is
 * as much on screen as one the user dragged there, and the search that could not
 * see it dropped the tutorial's first agent straight on top of End.
 */
export function fallbackNodeSpot(index: number): Point {
  return { x: 200, y: 80 + index * 140 };
}

function overlaps(a: Point, b: Point): boolean {
  return Math.abs(a.x - b.x) < STEP_X && Math.abs(a.y - b.y) < STEP_Y;
}

/**
 * The top-left corner for a new card whose ideal spot is `base`, moved aside
 * until it clears every existing card.
 */
export function freeDropSpot(base: Point, existing: Point[]): Point {
  const free = (spot: Point) => !existing.some((other) => overlaps(spot, other));

  let spot = base;
  for (let k = 1; k <= CANDIDATES && !free(spot); k++) {
    spot = {
      x: base.x + (k % COLUMNS) * STEP_X,
      y: base.y + Math.floor(k / COLUMNS) * STEP_Y,
    };
  }
  return spot;
}
