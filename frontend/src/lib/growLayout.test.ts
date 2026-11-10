import { describe, expect, it } from "vitest";
import { growBands, growShift, growUnshift } from "./growLayout";

describe("growLayout", () => {
  it("moves nothing when no box grows", () => {
    const bands = growBands([
      { id: "a", y: 0, h: 65, hGrown: 65 },
      { id: "b", y: 200, h: 65, hGrown: 65 },
    ]);
    expect([0, 64, 65, 300].map((y) => growShift(bands, y))).toEqual([0, 0, 0, 0]);
  });

  it("gives a grown card's extra height to everything under it, nothing above or level", () => {
    const bands = growBands([{ id: "a", y: 100, h: 65, hGrown: 97 }]);
    expect(growShift(bands, 0)).toBe(0);
    expect(growShift(bands, 100)).toBe(0);
    // Level with its side ports: the wire into it stays straight.
    expect(growShift(bands, 132)).toBe(0);
    expect(growShift(bands, 165)).toBe(32);
    expect(growShift(bands, 400)).toBe(32);
  });

  it("gives the largest growth of a row of overlapping cards, once", () => {
    const bands = growBands([
      { id: "a", y: 100, h: 65, hGrown: 81 },
      { id: "b", y: 110, h: 65, hGrown: 113 },
      { id: "c", y: 300, h: 65, hGrown: 81 },
    ]);
    expect(bands.map((b) => b.gain)).toEqual([48, 16]);
    expect(growShift(bands, 200)).toBe(48);
    expect(growShift(bands, 400)).toBe(64);
  });

  it("keeps the vertical order of every point", () => {
    const bands = growBands([
      { id: "a", y: 0, h: 65, hGrown: 113 },
      { id: "b", y: 100, h: 24, hGrown: 24 },
      { id: "c", y: 150, h: 65, hGrown: 81 },
    ]);
    let last = -Infinity;
    for (let y = -20; y < 400; y++) {
      const shown = y + growShift(bands, y);
      expect(shown).toBeGreaterThan(last);
      last = shown;
    }
  });

  it("finds where a drawn point is placed, the inverse of the shift", () => {
    const bands = growBands([
      { id: "a", y: 0, h: 65, hGrown: 113 },
      { id: "c", y: 150, h: 65, hGrown: 81 },
    ]);
    for (let y = -20; y < 400; y++) expect(growUnshift(bands, y + growShift(bands, y))).toBe(y);
  });

  it("places what is dropped in the strip a growth opens at the bottom of its row", () => {
    const bands = growBands([{ id: "a", y: 0, h: 65, hGrown: 97 }]);
    expect(growUnshift(bands, 80)).toBe(65);
    expect(growUnshift(bands, 97)).toBe(65);
    expect(growUnshift(bands, 98)).toBe(66);
  });
});
