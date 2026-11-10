import { describe, expect, it } from "vitest";
import { CANVAS_MIN_PCT, defaultPaneSizes, PANE_BOUNDS, restorePaneSizes } from "./paneSizes";

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

describe("restorePaneSizes", () => {
  const px = (pct: number, width: number) => (pct / 100) * width;

  it("keeps the side panels' pixels in a wider window", () => {
    const saved = { left: 25, center: 45, right: 30 }; // 300 / 360 px of 1200
    const sizes = restorePaneSizes(saved, 1200, 1600);
    expect(px(sizes.left, 1600)).toBeCloseTo(300, 0);
    expect(px(sizes.right, 1600)).toBeCloseTo(360, 0);
    expect(sizes.left + sizes.center + sizes.right).toBeCloseTo(100, 5);
  });

  it("keeps them at another UI scale (fewer CSS px in the same window)", () => {
    const saved = defaultPaneSizes(1440); // 260 px at 100 %
    const sizes = restorePaneSizes(saved, 1440, 1200); // the same window at 120 %
    expect(px(sizes.left, 1200)).toBeCloseTo(260, 0);
    expect(px(sizes.right, 1200)).toBeCloseTo(400, 0);
  });

  it("brings a width out of bounds back within them", () => {
    const sizes = restorePaneSizes({ left: 5, center: 55, right: 40 }, 2000, 2000);
    expect(px(sizes.left, 2000)).toBeCloseTo(200, 0);
    expect(px(sizes.right, 2000)).toBeCloseTo(720, 0);
  });

  it("restores side panels dragged to their maximum as dragged, the canvas under a third", () => {
    // 1440 px: 480 + 720 px of side panels leave the canvas 240 px, 16.67 %.
    const dragged = { left: (480 / 1440) * 100, center: (240 / 1440) * 100, right: (720 / 1440) * 100 };
    const sizes = restorePaneSizes(dragged, 1440, 1440);
    expect(px(sizes.left, 1440)).toBeCloseTo(480, 0);
    expect(px(sizes.right, 1440)).toBeCloseTo(720, 0);
    expect(sizes.center).toBeCloseTo(16.67, 1);
  });

  it("keeps the canvas its saved pixels in a narrower window, the side panels giving way", () => {
    const dragged = { left: (480 / 1440) * 100, center: (240 / 1440) * 100, right: (720 / 1440) * 100 };
    const sizes = restorePaneSizes(dragged, 1440, 1200);
    expect(px(sizes.center, 1200)).toBeCloseTo(240, 0);
    expect(sizes.left / sizes.right).toBeCloseTo(480 / 720, 2);
    expect(sizes.left + sizes.center + sizes.right).toBeCloseTo(100, 5);
  });

  it("never gives the canvas less than its minimum, the floor a drag stops at", () => {
    // 1200 px: 480 + 720 px of side panels would leave the canvas nothing.
    const sizes = restorePaneSizes({ left: 40, center: 0, right: 60 }, 1200, 1200);
    expect(sizes.center).toBeCloseTo(CANVAS_MIN_PCT, 1);
    expect(sizes.left + sizes.center + sizes.right).toBeCloseTo(100, 5);
  });

  it("gives an older layout, saved without its width, the defaults", () => {
    expect(restorePaneSizes({ left: 15, center: 60, right: 25 }, null, 1067)).toEqual(defaultPaneSizes(1067));
  });
});
