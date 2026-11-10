import { describe, it, expect } from "vitest";
import { rimBand, rimDepth } from "./rimBand";

const CARD = { width: 160, height: 35 };

describe("rimBand — the drag-source band, constant on screen (CORE-88)", () => {
  it("is 8px on screen at 25 %, 100 % and 200 %", () => {
    for (const zoom of [0.25, 1, 2]) {
      const band = rimBand(zoom, CARD.height);
      expect(band.width * zoom).toBeCloseTo(8);
      expect((band.inside + band.outside) * zoom).toBeCloseTo(8);
    }
  });

  it("lies at most 3px inside the card, the rest outside", () => {
    const band = rimBand(1, CARD.height);
    expect(band.inside).toBeCloseTo(3);
    expect(band.outside).toBeCloseTo(5);
    expect(rimBand(2, CARD.height).inside * 2).toBeCloseTo(3);
  });

  it("never takes more than 25 % of the card's depth, so a dezoomed card still moves", () => {
    // 35 × 0.25 = 8.75 screen px tall: 3px top + 3px bottom would leave 2.75px.
    const band = rimBand(0.25, CARD.height);
    expect(band.inside).toBeCloseTo(35 * 0.25);
    expect(band.inside * 0.25).toBeLessThan(3);
    expect(band.width * 0.25).toBeCloseTo(8);
  });

  it("measures the depth across the side", () => {
    expect(rimDepth("top", CARD)).toBe(35);
    expect(rimDepth("bottom", CARD)).toBe(35);
    expect(rimDepth("left", CARD)).toBe(160);
    expect(rimDepth("right", CARD)).toBe(160);
  });
});
