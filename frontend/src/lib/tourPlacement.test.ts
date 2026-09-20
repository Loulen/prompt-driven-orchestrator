/** Where the tour popover lands (#823) — the flip rules, checked without a DOM. */
import { describe, it, expect } from "vitest";
import { placePopover } from "./tourPlacement";

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
