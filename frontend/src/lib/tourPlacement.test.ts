/** Where the tour popover lands (#823) — the flip rules, checked without a DOM. */
import { describe, it, expect } from "vitest";
import { blockerRects, placePopover, type Box } from "./tourPlacement";

const VIEW = { width: 1000, height: 700 };
const CARD = { width: 260, height: 160 };

describe("placePopover", () => {
  it("sits to the right of the hole when there is room", () => {
    const p = placePopover({ top: 300, left: 100, width: 80, height: 40 }, CARD, VIEW);
    expect(p.side).toBe("right");
    expect(p.left).toBe(100 + 80 + 12);
    // Centred on the hole.
    expect(p.top).toBe(300 + 20 - 80);
  });

  it("flips to the left when the right edge is too close", () => {
    const p = placePopover({ top: 300, left: 900, width: 60, height: 40 }, CARD, VIEW);
    expect(p.side).toBe("left");
    expect(p.left).toBe(900 - 12 - 260);
  });

  it("drops below when neither side fits", () => {
    const p = placePopover({ top: 40, left: 0, width: 1000, height: 60 }, CARD, VIEW);
    expect(p.side).toBe("bottom");
    expect(p.top).toBe(40 + 60 + 12);
  });

  it("goes above when there is no room below either", () => {
    const p = placePopover({ top: 500, left: 0, width: 1000, height: 190 }, CARD, VIEW);
    expect(p.side).toBe("top");
    expect(p.top).toBe(500 - 12 - 160);
  });

  it("never leaves the viewport, whatever the hole does", () => {
    for (const hole of [
      { top: -50, left: 0, width: 40, height: 20 },
      { top: 690, left: 10, width: 40, height: 20 },
      { top: 300, left: -80, width: 40, height: 20 },
    ]) {
      const p = placePopover(hole, CARD, VIEW);
      expect(p.top).toBeGreaterThanOrEqual(0);
      expect(p.left).toBeGreaterThanOrEqual(0);
      expect(p.top + CARD.height).toBeLessThanOrEqual(VIEW.height);
      expect(p.left + CARD.width).toBeLessThanOrEqual(VIEW.width);
    }
  });

  it("centres itself when there is no hole to anchor to", () => {
    const p = placePopover(null, CARD, VIEW);
    expect(p.left).toBe((1000 - 260) / 2);
    expect(p.top).toBe((700 - 160) / 2);
  });
});

// #911 — several openings: the target and the read-only panel its gesture opened.
describe("blockerRects", () => {
  const VP = { width: 1000, height: 600 };

  /** Is the point dimmed — inside one of the blockers? */
  const dimmed = (rects: Box[], x: number, y: number) =>
    rects.some((r) => x >= r.left && x < r.left + r.width && y >= r.top && y < r.top + r.height);

  it("leaves every opening uncovered and dims everything else", () => {
    const card = { top: 100, left: 200, width: 120, height: 60 };
    const panel = { top: 0, left: 700, width: 300, height: 600 };
    const rects = blockerRects([card, panel], VP);
    for (const [x, y] of [
      [210, 110],
      [319, 159],
      [705, 5],
      [999, 599],
    ]) {
      expect(dimmed(rects, x, y), `${x},${y} is lit`).toBe(false);
    }
    for (const [x, y] of [
      [0, 0],
      [199, 110],
      [210, 99],
      [210, 161],
      [500, 300],
      [699, 300],
    ]) {
      expect(dimmed(rects, x, y), `${x},${y} is dimmed`).toBe(true);
    }
  });

  it("never lets two blockers overlap, so the dim is even", () => {
    const rects = blockerRects(
      [
        { top: 100, left: 100, width: 200, height: 200 },
        { top: 150, left: 250, width: 200, height: 100 },
      ],
      VP,
    );
    const area = rects.reduce((sum, r) => sum + r.width * r.height, 0);
    // The viewport minus the union of the two openings (40000 + 20000 - 5000 overlap).
    expect(area).toBe(1000 * 600 - (40000 + 20000 - 50 * 100));
  });

  it("merges neighbouring stripes: a panel against the window's edge costs few blockers", () => {
    const rects = blockerRects([{ top: 0, left: 700, width: 300, height: 600 }], VP);
    expect(rects).toEqual([{ top: 0, left: 0, width: 700, height: 600 }]);
  });

  it("clips openings that run off the window", () => {
    const rects = blockerRects([{ top: -50, left: -50, width: 100, height: 100 }], VP);
    expect(dimmed(rects, 10, 10)).toBe(false);
    expect(dimmed(rects, 60, 10)).toBe(true);
    for (const r of rects) {
      expect(r.left).toBeGreaterThanOrEqual(0);
      expect(r.top).toBeGreaterThanOrEqual(0);
    }
  });

  it("dims the whole window when there is nothing to leave open", () => {
    expect(blockerRects([], VP)).toEqual([{ top: 0, left: 0, width: 1000, height: 600 }]);
  });
});
