/**
 * The vertical growth of a canvas: « Compact nodes » the other way round
 * (`compactLayout.ts`). Pure geometry: it reads rectangles and returns how far
 * each ordinate goes down; nothing is written back.
 *
 * A card taller than the layout it was placed for (a node card with several
 * outputs) gives its extra height to everything below it. Boxes whose vertical
 * extents overlap form a band, and a band gives the LARGEST growth of its
 * boxes, so no grown card reaches into what lies under it. The shift is one
 * function of `y` alone, non-decreasing: a horizontal segment stays horizontal,
 * a vertical one keeps its direction, and no edge gains a bend. Inside a band
 * nothing moves but by what the bands above gave; from its bottom on,
 * everything also takes its growth.
 */

/** A box of the canvas, in flow px: its top, its height as placed, and as drawn. */
export interface GrowRect {
  id: string;
  y: number;
  /** Its height in the layout it was placed for. */
  h: number;
  /** Its height as drawn: `h` for what does not grow. */
  hGrown: number;
}

export interface GrowBand {
  top: number;
  /** The bottom of its boxes as placed. */
  bottom: number;
  /** The largest growth of its boxes. */
  gain: number;
  /** The shift above the band. */
  before: number;
}

/** The bands: boxes whose vertical extents (as placed) overlap, chained, top to bottom. */
export function growBands(rects: readonly GrowRect[]): GrowBand[] {
  const sorted = [...rects].sort((a, b) => a.y - b.y);
  const out: GrowBand[] = [];
  let cur: GrowBand | null = null;
  for (const r of sorted) {
    const gain = Math.max(0, r.hGrown - r.h);
    if (cur && r.y < cur.bottom) {
      cur.bottom = Math.max(cur.bottom, r.y + r.h);
      cur.gain = Math.max(cur.gain, gain);
      continue;
    }
    if (cur) out.push(cur);
    cur = { top: r.y, bottom: r.y + r.h, gain, before: 0 };
  }
  if (cur) out.push(cur);
  let acc = 0;
  for (const b of out) {
    b.before = acc;
    acc += b.gain;
  }
  return out;
}

/** How far a point placed at `y` goes down: the growth of every band whose bottom is at or above it. */
export function growShift(bands: readonly GrowBand[], y: number): number {
  for (const b of bands) if (y < b.bottom) return b.before;
  const last = bands[bands.length - 1];
  return last ? last.before + last.gain : 0;
}

/**
 * Where a point drawn at `y` is placed: the inverse of {@link growShift}. The
 * strip a band's growth opens under it maps to the band's bottom, so what is
 * dropped there lands under the grown cards. The strip spans the whole width
 * (the shift knows only `y`): a segment or a card dragged through it snaps to
 * its lower edge.
 */
export function growUnshift(bands: readonly GrowBand[], y: number): number {
  for (const b of bands) {
    if (y < b.bottom + b.before) return y - b.before;
    if (y < b.bottom + b.before + b.gain) return b.bottom;
  }
  const last = bands[bands.length - 1];
  return last ? y - last.before - last.gain : y;
}
