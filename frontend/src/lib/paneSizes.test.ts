import { describe, expect, it } from "vitest";
import { defaultPaneSizes, PANE_BOUNDS } from "./paneSizes";

describe("defaultPaneSizes", () => {
  it("gives the left panel 260 px and the inspector 400 px of the window, the canvas the rest", () => {
    // 1280 px at 120 %: 1067 CSS px.
    const sizes = defaultPaneSizes(1067);
    expect(sizes.left).toBeCloseTo((260 / 1067) * 100, 1);
    expect(sizes.right).toBeCloseTo((400 / 1067) * 100, 1);
    expect(sizes.left + sizes.center + sizes.right).toBeCloseTo(100, 5);
  });

  it("keeps the canvas at least a third of a narrow window", () => {
    const sizes = defaultPaneSizes(600);
    expect(sizes.center).toBeGreaterThanOrEqual(33);
    expect(sizes.left + sizes.center + sizes.right).toBeCloseTo(100, 5);
  });

  it("falls back to fixed shares without a width", () => {
    expect(defaultPaneSizes(0)).toEqual({ left: 20, center: 55, right: 25 });
  });

  it("bounds both side panels in pixels", () => {
    expect(PANE_BOUNDS.left).toEqual({ min: 200, max: 480 });
    expect(PANE_BOUNDS.right).toEqual({ min: 240, max: 720 });
  });
});
