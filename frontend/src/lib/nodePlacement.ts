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
 *
 * And it walks that grid **inside the visible canvas**, because the grid alone
 * fixed one half of the bug and opened the other (the FP again, iteration 2). A
 * step is 220 canvas units; on a fresh pipeline the canvas fit-views two markers
 * at zoom 2, so one step right is 440 screen pixels and the second card the tour
 * has the reader create lands off the edge of the canvas — visible nowhere,
 * clickable by nothing, with a tour asking for it. A card that does not overlap
 * but cannot be seen is not placed. Hence {@link Rect}: the drop search is given
 * the visible region in canvas units and only ever answers with a spot that fits
 * inside it, falling back to the blind walk when the screen is genuinely full.
 */

export interface Point {
  x: number;
  y: number;
}

/** A region of the canvas, in canvas units — here, the part of it on screen. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Approximate default node-card footprint; nodes auto-size around this. */
export const CARD_WIDTH = 208;
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
/** How far the on-screen search reaches from the drop point, in grid steps. A
 *  canvas zoomed far out shows hundreds of slots; the one we want is near the
 *  middle, and 8 steps each way is already 289 candidates. */
const SPAN = 8;

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

/** The whole card fits within the visible region — not merely its corner. */
function fitsInside(spot: Point, visible: Rect): boolean {
  return (
    spot.x >= visible.x &&
    spot.y >= visible.y &&
    spot.x + CARD_WIDTH <= visible.x + visible.width &&
    spot.y + CARD_HEIGHT <= visible.y + visible.height
  );
}

/**
 * Every slot of the grid that fits on screen, nearest the drop point first, and
 * within a row: right before left, because a pipeline reads left to right. The
 * grid is anchored on `base` rather than on the viewport, so the answer is still
 * « where the user dropped it, moved aside » and not « wherever there is room ».
 *
 * `withEdges` adds, on each axis, the two places a card sits flush against the
 * visible edges. A grid anchored on the drop point leaves a strip less than one
 * step wide on each side: with the right pane open, a 614-unit-wide canvas fits
 * a single column of the grid, and once that column was full the card walked
 * blind off the right edge, under the pane, with room left of Start all along.
 */
function onScreenCandidates(base: Point, visible: Rect, withEdges = false): Point[] {
  const steps = (lo: number, hi: number, step: number, origin: number): number[] => {
    const out: number[] = [];
    for (let k = Math.max(-SPAN, Math.ceil(lo)); k <= Math.min(SPAN, Math.floor(hi)); k++) out.push(origin + k * step);
    return out;
  };
  const left = visible.x;
  const right = visible.x + visible.width - CARD_WIDTH;
  const top = visible.y;
  const bottom = visible.y + visible.height - CARD_HEIGHT;
  const xs = steps((left - base.x) / STEP_X, (right - base.x) / STEP_X, STEP_X, base.x);
  const ys = steps((top - base.y) / STEP_Y, (bottom - base.y) / STEP_Y, STEP_Y, base.y);
  if (withEdges) {
    // Rounded inward: a card position is whole units, and still fits.
    xs.push(Math.ceil(left), Math.floor(right));
    ys.push(Math.ceil(top), Math.floor(bottom));
  }

  // Nearest slot first (rings around the drop point), and within a ring: to the
  // right before anywhere else, then down, then up, and only then back to the
  // left. A pipeline reads left to right, so a card placed behind its neighbour
  // is the last acceptable answer — but it beats a card nobody can see.
  const out: { spot: Point; i: number; j: number }[] = [];
  const seen = new Set<string>();
  for (const y of ys) {
    for (const x of xs) {
      const spot = { x, y };
      const key = `${spot.x},${spot.y}`;
      if (seen.has(key) || !fitsInside(spot, visible)) continue;
      seen.add(key);
      out.push({ spot, i: (spot.x - base.x) / STEP_X, j: (spot.y - base.y) / STEP_Y });
    }
  }
  return out
    .sort(
      (a, b) =>
        Math.abs(a.i) + Math.abs(a.j) - (Math.abs(b.i) + Math.abs(b.j)) ||
        b.i - a.i ||
        b.j - a.j,
    )
    .map((c) => c.spot);
}

/** The card at `spot` covers part of `area`. */
function covers(spot: Point, area: Rect): boolean {
  return (
    spot.x < area.x + area.width &&
    spot.x + CARD_WIDTH > area.x &&
    spot.y < area.y + area.height &&
    spot.y + CARD_HEIGHT > area.y
  );
}

/**
 * The top-left corner for a new card whose ideal spot is `base`, moved aside
 * until it clears every existing card — and, when the caller can say what part
 * of the canvas is on screen, kept inside it.
 *
 * `visible` omitted is the honest answer for a caller with no canvas to measure
 * (a test, a canvas not laid out yet): the search then walks blind, which is
 * what it did before it could see.
 *
 * `avoided`: areas a new card must not cover either — a loop region's box,
 * whose corner counter would sit over a card that is not one of its members.
 */
export function freeDropSpot(base: Point, existing: Point[], visible?: Rect | null, avoided: Rect[] = []): Point {
  const free = (spot: Point) =>
    !existing.some((other) => overlaps(spot, other)) && !avoided.some((area) => covers(spot, area));

  if (visible && visible.width > 0 && visible.height > 0) {
    // The grid first, so the answer stays « the drop point, moved aside »; the
    // slots flush against the edges only when the grid has none left.
    const onScreen =
      onScreenCandidates(base, visible).find(free) ?? onScreenCandidates(base, visible, true).find(free);
    // No free slot on screen: the visible canvas is genuinely full, and every
    // answer is a compromise. Not overlapping wins over being in frame — the
    // reader can pan to a card, they cannot click one under another.
    if (onScreen) return onScreen;
  }

  let spot = base;
  for (let k = 1; k <= CANDIDATES && !free(spot); k++) {
    spot = {
      x: base.x + (k % COLUMNS) * STEP_X,
      y: base.y + Math.floor(k / COLUMNS) * STEP_Y,
    };
  }
  return spot;
}
