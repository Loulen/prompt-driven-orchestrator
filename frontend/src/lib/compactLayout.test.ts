import { describe, expect, it } from "vitest";
import { boxesOverlap, compactBands, compactLayout, compactShift, type CompactRect } from "./compactLayout";

/** A seeded generator (mulberry32): every run draws the same layouts. */
function random(seed: number) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Columns of boxes that never overlap: cards, two-line cards, markers and notes. */
function layoutOf(seed: number): CompactRect[] {
  const rnd = random(seed);
  const rects: CompactRect[] = [];
  const columns = 1 + Math.floor(rnd() * 5);
  for (let c = 0; c < columns; c++) {
    let y = Math.floor(rnd() * 200);
    const count = 1 + Math.floor(rnd() * 8);
    for (let i = 0; i < count; i++) {
      const kind = rnd();
      const [h, hCompact] = kind < 0.6 ? [65, 30] : kind < 0.75 ? [87, 52] : kind < 0.9 ? [24, 24] : [40 + Math.floor(rnd() * 100), 0];
      const x = c * 260 + Math.floor(rnd() * 30);
      rects.push({ id: `${c}-${i}`, x, y, w: 208, h, hCompact: hCompact || h });
      y += h + 10 + Math.floor(rnd() * 120);
    }
  }
  return rects;
}

const inside = (p: { x: number; y: number }, r: { x: number; y: number; w: number; h: number }) =>
  p.x > r.x && p.x < r.x + r.w && p.y > r.y && p.y < r.y + r.h;

describe("compactLayout", () => {
  it("lets no two boxes overlap, over 3000 random layouts", () => {
    const failures: number[] = [];
    for (let seed = 1; seed <= 3000; seed++) {
      const out = Object.values(compactLayout(layoutOf(seed)).rects);
      for (let i = 0; i < out.length; i++)
        for (let j = i + 1; j < out.length; j++) if (boxesOverlap(out[i], out[j])) failures.push(seed);
    }
    expect(failures).toEqual([]);
  });

  it("keeps the vertical order, so a horizontal segment stays horizontal and none gains a bend", () => {
    for (let seed = 1; seed <= 500; seed++) {
      const rects = layoutOf(seed);
      const bands = compactBands(rects);
      const ys = Array.from({ length: 200 }, (_, i) => i * 7 - 50);
      const moved = ys.map((y) => y - compactShift(bands, y));
      expect(moved.every((y, i) => i === 0 || y >= moved[i - 1]), `seed ${seed}`).toBe(true);
      // A segment's two ends at one ordinate share one shift.
      const { points } = compactLayout(rects, [
        { id: "a", x: 10, y: 333 },
        { id: "b", x: 900, y: 333 },
      ]);
      expect(points.a.y).toBe(points.b.y);
    }
  });

  it("puts no label on a box when it was on none", () => {
    for (let seed = 1; seed <= 500; seed++) {
      const rects = layoutOf(seed);
      const rnd = random(seed * 7);
      const labels = Array.from({ length: 40 }, (_, i) => ({ id: `l${i}`, x: Math.floor(rnd() * 1300), y: Math.floor(rnd() * 1500) })).filter(
        (p) => !rects.some((r) => inside(p, r)),
      );
      const out = compactLayout(rects, labels);
      const covered = labels.filter((p) => Object.values(out.rects).some((r) => inside(out.points[p.id], r)));
      expect(covered, `seed ${seed}`).toEqual([]);
    }
  });

  it("is the identity when nothing shrinks: off again is the input", () => {
    for (let seed = 1; seed <= 200; seed++) {
      const rects = layoutOf(seed).map((r) => ({ ...r, hCompact: r.h }));
      const out = compactLayout(rects, [{ id: "p", x: 3, y: 777 }]);
      for (const r of rects) expect(out.rects[r.id]).toEqual({ x: r.x, y: r.y, w: r.w, h: r.h });
      expect(out.points.p).toEqual({ x: 3, y: 777 });
    }
  });

  it("raises a band of side-by-side cards of different heights by its smallest gain", () => {
    const rects: CompactRect[] = [
      { id: "top", x: 0, y: 0, w: 208, h: 65, hCompact: 30 },
      { id: "plain", x: 0, y: 100, w: 208, h: 65, hCompact: 30 },
      { id: "orch", x: 260, y: 100, w: 208, h: 87, hCompact: 52 },
      { id: "tall", x: 520, y: 100, w: 208, h: 120, hCompact: 30 },
      { id: "below", x: 0, y: 300, w: 208, h: 65, hCompact: 30 },
    ];
    const out = compactLayout(rects);
    expect(compactBands(rects).map((b) => b.gain)).toEqual([35, 35, 35]);
    expect(out.rects.plain.y).toBe(65);
    expect(out.rects.orch.y).toBe(65);
    expect(out.rects.tall.y).toBe(65);
    expect(out.rects.below.y).toBe(300 - 70);
  });

  it("holds a band that a box which does not shrink shares", () => {
    const rects: CompactRect[] = [
      { id: "a", x: 0, y: 0, w: 208, h: 65, hCompact: 30 },
      { id: "end", x: 300, y: 0, w: 60, h: 24, hCompact: 24 },
      { id: "b", x: 0, y: 100, w: 208, h: 65, hCompact: 30 },
    ];
    const out = compactLayout(rects);
    expect(out.rects.b.y).toBe(100);
  });
});
