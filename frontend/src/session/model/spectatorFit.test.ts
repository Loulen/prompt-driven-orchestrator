import { describe, it, expect } from "vitest";
import { spectatorFontSize, BASE_FONT_SIZE, MIN_SPECTATOR_FONT_SIZE } from "./spectatorFit";

// An area of `w × h` px where a cell is 0.6 × 1.2 font px, like a monospace font.
const area = (w: number, h: number) => (font: number) => ({
  cols: Math.floor(w / (font * 0.6)),
  rows: Math.floor(h / (font * 1.2)),
});

describe("spectatorFontSize (#869)", () => {
  it("keeps the normal font when the pilot's grid already fits", () => {
    expect(spectatorFontSize({ cols: 80, rows: 24 }, area(1000, 500))).toBe(BASE_FONT_SIZE);
  });

  it("never grows past the normal font", () => {
    expect(spectatorFontSize({ cols: 10, rows: 5 }, area(5000, 5000))).toBe(BASE_FONT_SIZE);
  });

  it("shrinks to the largest size at which the whole grid fits", () => {
    const measure = area(1000, 500);
    const size = spectatorFontSize({ cols: 200, rows: 50 }, measure);
    expect(size).toBeLessThan(BASE_FONT_SIZE);
    const at = measure(size);
    expect(at.cols).toBeGreaterThanOrEqual(200);
    expect(at.rows).toBeGreaterThanOrEqual(50);
    // Half a point bigger would not fit.
    const bigger = measure(size + 0.5);
    expect(bigger.cols < 200 || bigger.rows < 50).toBe(true);
  });

  it("stops at the readability floor", () => {
    expect(spectatorFontSize({ cols: 400, rows: 200 }, area(300, 150))).toBe(MIN_SPECTATOR_FONT_SIZE);
  });

  it("keeps the normal font while the area cannot be measured", () => {
    expect(spectatorFontSize({ cols: 200, rows: 50 }, () => undefined)).toBe(BASE_FONT_SIZE);
  });
});
