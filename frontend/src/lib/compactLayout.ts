/**
 * The vertical compaction of a canvas (« Nodes compactes »): pure geometry, it
 * reads rectangles and points and returns where they go. Nothing is written
 * back; with compaction off the caller keeps its input, untouched.
 *
 * Each card gives the height of its details (detailed height minus compact
 * height) to everything below it. Cards whose vertical extents overlap form a
 * band, and a band gives the smallest gain of its cards, so no card rises into
 * another. The shift is one function of `y` alone, continuous, non-decreasing,
 * with a slope of at most 1 (a ramp across each band's details): `y - shift(y)`
 * keeps the vertical order of everything, so a horizontal segment stays
 * horizontal, a vertical one keeps its direction, and no edge gains a bend. A
 * box that does not shrink (a gain of 0) holds its band where it is.
 */

/** A box of the canvas, in flow px. `y` is its top. */
export interface CompactRect {
  id: string;
  x: number;
  y: number;
  w: number;
  /** Its height in the detailed layout. */
  h: number;
  /** Its height once compacted: `h` for what does not shrink. */
  hCompact: number;
}

/** A point that follows the canvas: a waypoint, a label, a note's corner. */
export interface CompactPoint {
  id: string;
  x: number;
  y: number;
}

export interface CompactBand {
  top: number;
  bottom: number;
  /** The smallest gain of its boxes. */
  gain: number;
  /** The shift above the band. */
  before: number;
  /** Where its ramp starts: its top plus its shortest compact height. */
  rampFrom: number;
}

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface CompactLayout {
  /** The compacted boxes: same `x` and width, new top (whole px), compact height. */
  rects: Record<string, Box>;
  points: Record<string, { x: number; y: number }>;
  bands: CompactBand[];
}

/** The bands: boxes whose vertical extents overlap, chained, top to bottom. */
export function compactBands(rects: readonly CompactRect[]): CompactBand[] {
  const sorted = [...rects].sort((a, b) => a.y - b.y);
  const out: CompactBand[] = [];
  let cur: { top: number; bottom: number; gain: number; hc: number } | null = null;
  const close = () => {
    if (cur) out.push({ top: cur.top, bottom: cur.bottom, gain: cur.gain, before: 0, rampFrom: cur.top + cur.hc });
  };
  for (const r of sorted) {
    const gain = Math.max(0, r.h - r.hCompact);
    if (cur && r.y < cur.bottom) {
      cur.bottom = Math.max(cur.bottom, r.y + r.h);
      cur.gain = Math.min(cur.gain, gain);
      cur.hc = Math.min(cur.hc, r.hCompact);
      continue;
    }
    close();
    cur = { top: r.y, bottom: r.y + r.h, gain, hc: r.hCompact };
  }
  close();
  let acc = 0;
  for (const b of out) {
    b.before = acc;
    acc += b.gain;
  }
  return out;
}

/** How far a point at `y` rises: the gains of the bands above it, plus a linear share of the band it falls in. */
export function compactShift(bands: readonly CompactBand[], y: number): number {
  for (const b of bands) {
    if (y >= b.bottom) continue;
    if (y > b.rampFrom && b.bottom > b.rampFrom) return b.before + (b.gain * (y - b.rampFrom)) / (b.bottom - b.rampFrom);
    return b.before;
  }
  const last = bands[bands.length - 1];
  return last ? last.before + last.gain : 0;
}

export function compactLayout(rects: readonly CompactRect[], points: readonly CompactPoint[] = []): CompactLayout {
  const bands = compactBands(rects);
  const out: CompactLayout = { rects: {}, points: {}, bands };
  // Rounded to whole flow px, like every position the canvas draws: rounding
  // keeps the order, so what the shift guarantees still holds.
  for (const r of rects) out.rects[r.id] = { x: r.x, y: Math.round(r.y - compactShift(bands, r.y)), w: r.w, h: r.hCompact };
  for (const p of points) out.points[p.id] = { x: p.x, y: Math.round(p.y - compactShift(bands, p.y)) };
  return out;
}

/** Whether two boxes overlap (their borders may touch). */
export function boxesOverlap(a: Box, b: Box): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}
