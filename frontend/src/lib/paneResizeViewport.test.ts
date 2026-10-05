import { describe, expect, it } from "vitest";
import { reframeOnPaneResize } from "./paneResizeViewport";

describe("reframeOnPaneResize (#949)", () => {
  it("keeps the graph's visual centre when the pane unfolds", () => {
    // Canvas 1400 -> 800: the centre moves 300px left, so does the graph.
    expect(reframeOnPaneResize({ x: 100, y: 50, zoom: 1 }, 1400, 800, 600, null)).toEqual({
      x: -200,
      y: 50,
      zoom: 1,
    });
  });

  it("keeps the centre when the pane folds back", () => {
    expect(reframeOnPaneResize({ x: -200, y: 50, zoom: 0.5 }, 800, 1400, 600, null)).toEqual({
      x: 100,
      y: 50,
      zoom: 0.5,
    });
  });

  it("leaves the centred frame alone when the selected card is in it", () => {
    const card = { x: 300, y: 100, width: 200, height: 100 };
    // After the shift: x=-200 -> card spans 100..300 on screen, inside 800.
    expect(reframeOnPaneResize({ x: 100, y: 0, zoom: 1 }, 1400, 800, 600, card)).toEqual({
      x: -200,
      y: 0,
      zoom: 1,
    });
  });

  it("centres on the selected card when the shift still leaves it cut off", () => {
    // After the shift the card spans 900..1100 on an 800px canvas.
    const card = { x: 1000, y: 100, width: 200, height: 100 };
    expect(reframeOnPaneResize({ x: 200, y: 0, zoom: 1 }, 1400, 800, 600, card)).toEqual({
      x: 400 - 1100,
      y: 300 - 150,
      zoom: 1,
    });
  });

  it("honours the zoom when centring on the card", () => {
    const card = { x: 3000, y: 0, width: 200, height: 100 };
    const v = reframeOnPaneResize({ x: 0, y: 0, zoom: 0.5 }, 1400, 800, 600, card);
    expect(v).toEqual({ x: 400 - 1550, y: 300 - 25, zoom: 0.5 });
  });

  it("stops the leftward pan where the graph meets the canvas's left edge", () => {
    // Graph panned left by the user: its left edge sits 100px into the canvas.
    // A 300px pan would push 200px of it out; the pan stops at 100px.
    const graph = { x: 0, y: 0, width: 500, height: 100 };
    expect(reframeOnPaneResize({ x: 100, y: 0, zoom: 1 }, 1400, 800, 600, null, graph)).toEqual({
      x: 0,
      y: 0,
      zoom: 1,
    });
  });

  it("does not pan left at all when the graph's left edge is already cut off", () => {
    const graph = { x: 0, y: 0, width: 500, height: 100 };
    expect(reframeOnPaneResize({ x: -50, y: 0, zoom: 1 }, 1400, 800, 600, null, graph)).toEqual({
      x: -50,
      y: 0,
      zoom: 1,
    });
  });

  it("keeps the full half-width pan for a centred graph", () => {
    // Graph spans 450..950 on a 1400px canvas: a 300px pan keeps it in frame.
    const graph = { x: 0, y: 0, width: 500, height: 100 };
    expect(reframeOnPaneResize({ x: 450, y: 0, zoom: 1 }, 1400, 800, 600, null, graph).x).toBe(150);
  });

  it("measures the graph's left edge at the current zoom", () => {
    // Left edge on screen: 400 * 0.5 - 100 = 100px, so the pan stops at 100px.
    const graph = { x: 400, y: 0, width: 1000, height: 100 };
    expect(reframeOnPaneResize({ x: -100, y: 0, zoom: 0.5 }, 1400, 800, 600, null, graph).x).toBe(-200);
  });

  it("does not bound the pan when the pane folds back", () => {
    const graph = { x: 0, y: 0, width: 500, height: 100 };
    expect(reframeOnPaneResize({ x: 0, y: 0, zoom: 1 }, 800, 1400, 600, null, graph).x).toBe(300);
  });
});
