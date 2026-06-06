import { describe, it, expect } from "vitest";
import { pathToSvg, segmentHandles, dragSegment } from "./edgePath";
import type { Point } from "./orthogonalRouter";

describe("pathToSvg", () => {
  it("renders a polyline as an SVG move + line commands", () => {
    const pts: Point[] = [
      { x: 0, y: 0 },
      { x: 100, y: 0 },
      { x: 100, y: 50 },
    ];
    expect(pathToSvg(pts)).toBe("M 0,0 L 100,0 L 100,50");
  });

  it("returns an empty string for fewer than two points", () => {
    expect(pathToSvg([{ x: 1, y: 1 }])).toBe("");
    expect(pathToSvg([])).toBe("");
  });
});

describe("segmentHandles", () => {
  it("places one handle at the midpoint of each segment with its orientation", () => {
    const pts: Point[] = [
      { x: 0, y: 0 }, // -> horizontal segment
      { x: 100, y: 0 }, // -> vertical segment
      { x: 100, y: 80 },
    ];
    const handles = segmentHandles(pts);
    expect(handles).toHaveLength(2);

    // Segment 0 is horizontal: its handle drags vertically (perpendicular).
    expect(handles[0]).toMatchObject({
      segmentIndex: 0,
      x: 50,
      y: 0,
      orientation: "horizontal",
    });
    // Segment 1 is vertical: its handle drags horizontally (perpendicular).
    expect(handles[1]).toMatchObject({
      segmentIndex: 1,
      x: 100,
      y: 40,
      orientation: "vertical",
    });
  });
});

describe("dragSegment", () => {
  // The base auto path between two offset endpoints: an HVH step.
  const base: Point[] = [
    { x: 0, y: 0 },
    { x: 100, y: 0 },
    { x: 100, y: 80 },
    { x: 200, y: 80 },
  ];

  it("moves a vertical segment horizontally, keeping endpoints anchored", () => {
    // Drag segment 1 (the vertical run at x=100) to x=140.
    const pinned = dragSegment(base, 1, 140);

    // Endpoints unchanged (anchored to their nodes).
    expect(pinned[0]).toEqual({ x: 0, y: 0 });
    expect(pinned[pinned.length - 1]).toEqual({ x: 200, y: 80 });
    // Both ends of the dragged vertical segment now sit at x=140.
    expect(pinned[1].x).toBe(140);
    expect(pinned[2].x).toBe(140);
    // The path stays orthogonal.
    for (let i = 1; i < pinned.length; i++) {
      const dx = Math.abs(pinned[i].x - pinned[i - 1].x);
      const dy = Math.abs(pinned[i].y - pinned[i - 1].y);
      expect(dx < 1e-6 || dy < 1e-6).toBe(true);
    }
  });

  it("moves a horizontal segment vertically", () => {
    // Drag segment 0 (horizontal run at y=0) down to y=30.
    const pinned = dragSegment(base, 0, 30);
    expect(pinned[0]).toEqual({ x: 0, y: 0 });
    expect(pinned[1].y).toBe(30);
  });
});
