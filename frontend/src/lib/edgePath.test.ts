import { describe, it, expect } from "vitest";
import {
  pathToSvg,
  segmentHandles,
  dragSegment,
  reanchorWaypoints,
  segHandleStyle,
  deleteWaypoint,
  connectionPreviewPath,
} from "./edgePath";
import type { Point } from "./orthogonalRouter";

// Asserts every consecutive pair in `[source, ...waypoints, target]` shares an
// x or a y (no diagonal segment).
function expectOrthogonal(source: Point, waypoints: Point[], target: Point) {
  const pts = [source, ...waypoints, target];
  for (let i = 1; i < pts.length; i++) {
    const dx = Math.abs(pts[i].x - pts[i - 1].x);
    const dy = Math.abs(pts[i].y - pts[i - 1].y);
    expect(dx < 1e-6 || dy < 1e-6).toBe(true);
  }
}

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

describe("segHandleStyle", () => {
  it("paints the handle in the edge's own color, not a hardcoded accent green", () => {
    const style = segHandleStyle(50, 0, "horizontal", "#f59e0b");
    expect(style.background).toBe("#f59e0b");
    // The old chunky green default must be gone.
    expect(style.background).not.toBe("var(--color-acc, #10b981)");
    expect(style.background).not.toBe("#10b981");
  });

  it("renders a thin handle aligned to the segment axis (long along the segment, thin across it)", () => {
    // A horizontal segment: the handle is dragged vertically, so it lies ALONG
    // the segment (wide in x) and stays thin across it (short in y).
    const h = segHandleStyle(0, 0, "horizontal", "#fff");
    expect(Number(h.width)).toBeGreaterThan(Number(h.height));
    // Thinner than the old 8px chunky bar.
    expect(Number(h.height)).toBeLessThan(8);

    // A vertical segment is the transpose: tall in y, thin in x.
    const v = segHandleStyle(0, 0, "vertical", "#fff");
    expect(Number(v.height)).toBeGreaterThan(Number(v.width));
    expect(Number(v.width)).toBeLessThan(8);
  });

  it("anchors the handle at the given midpoint and keeps the perpendicular resize cursor", () => {
    const h = segHandleStyle(50, 12, "horizontal", "#fff");
    expect(h.transform).toContain("translate(50px, 12px)");
    expect(h.cursor).toBe("ns-resize");

    const v = segHandleStyle(50, 12, "vertical", "#fff");
    expect(v.cursor).toBe("ew-resize");
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

  it("keeps both endpoints anchored when the dragged segment touches both (straight 2-point edge)", () => {
    // A straight aligned edge renders as exactly two points (one segment that
    // touches source and target). Dragging it must pin a route without moving
    // either node-anchored endpoint — a bend is inserted on each side.
    const straight: Point[] = [
      { x: 0, y: 0 },
      { x: 200, y: 0 },
    ];
    const pinned = dragSegment(straight, 0, 30);

    // Both endpoints stay put.
    expect(pinned[0]).toEqual({ x: 0, y: 0 });
    expect(pinned[pinned.length - 1]).toEqual({ x: 200, y: 0 });
    // The interior run now sits at the dragged coordinate.
    expect(pinned[1]).toEqual({ x: 0, y: 30 });
    expect(pinned[2]).toEqual({ x: 200, y: 30 });
    // And the whole path stays orthogonal.
    for (let i = 1; i < pinned.length; i++) {
      const dx = Math.abs(pinned[i].x - pinned[i - 1].x);
      const dy = Math.abs(pinned[i].y - pinned[i - 1].y);
      expect(dx < 1e-6 || dy < 1e-6).toBe(true);
    }
  });

  it("keeps both endpoints anchored for a vertical 2-point edge", () => {
    const straight: Point[] = [
      { x: 0, y: 0 },
      { x: 0, y: 200 },
    ];
    const pinned = dragSegment(straight, 0, 40);
    expect(pinned[0]).toEqual({ x: 0, y: 0 });
    expect(pinned[pinned.length - 1]).toEqual({ x: 0, y: 200 });
    expect(pinned[1]).toEqual({ x: 40, y: 0 });
    expect(pinned[2]).toEqual({ x: 40, y: 200 });
  });
});

describe("connectionPreviewPath", () => {
  it("produces an orthogonal SVG path (straight runs only, no bezier curve)", () => {
    const d = connectionPreviewPath({ x: 0, y: 0 }, { x: 200, y: 80 });
    // Orthogonal paths are M/L commands; a bezier preview would carry a `C`.
    expect(d).toMatch(/^M /);
    expect(d).toContain("L");
    expect(d).not.toContain("C");
  });

  it("starts at the source and ends at the target", () => {
    const d = connectionPreviewPath({ x: 10, y: 20 }, { x: 110, y: 60 });
    expect(d.startsWith("M 10,20")).toBe(true);
    expect(d.trimEnd().endsWith("110,60")).toBe(true);
  });
});

describe("deleteWaypoint", () => {
  it("removes the only waypoint, leaving none (caller reverts the edge to auto)", () => {
    const out = deleteWaypoint([{ x: 0, y: 80 }], 0);
    expect(out).toEqual([]);
  });

  it("removes the waypoint at the given index, keeping the others in order", () => {
    const wps: Point[] = [
      { x: 50, y: 0 },
      { x: 50, y: 40 },
      { x: 150, y: 40 },
    ];
    expect(deleteWaypoint(wps, 1)).toEqual([
      { x: 50, y: 0 },
      { x: 150, y: 40 },
    ]);
  });

  it("does not mutate the input array", () => {
    const wps: Point[] = [
      { x: 50, y: 0 },
      { x: 50, y: 40 },
    ];
    const copy = wps.map((p) => ({ ...p }));
    deleteWaypoint(wps, 0);
    expect(wps).toEqual(copy);
  });

  it("is a no-op for an out-of-range index", () => {
    const wps: Point[] = [{ x: 50, y: 0 }];
    expect(deleteWaypoint(wps, 5)).toEqual(wps);
    expect(deleteWaypoint(wps, -1)).toEqual(wps);
  });
});

describe("reanchorWaypoints", () => {
  // A manual route pinned while source sat at (0,0) and target at (200,80):
  //   source(0,0) -> w0(100,0) -> w1(100,80) -> target(200,80)
  // segment source->w0 is horizontal, w0->w1 vertical, w1->target horizontal.
  const waypoints: Point[] = [
    { x: 100, y: 0 },
    { x: 100, y: 80 },
  ];

  it("re-anchors the source-adjacent waypoint when the source node moves", () => {
    // Source node dragged down/left to (-40, 30). Naively the path would have a
    // diagonal source(-40,30)->w0(100,0). The first segment was horizontal, so
    // w0 must follow the source's new y to stay horizontal.
    const source: Point = { x: -40, y: 30 };
    const target: Point = { x: 200, y: 80 };

    const out = reanchorWaypoints(source, target, waypoints);

    expectOrthogonal(source, out, target);
    // w0 tracked the source on the shared (y) axis; its x is untouched.
    expect(out[0]).toEqual({ x: 100, y: 30 });
  });

  it("re-anchors the target-adjacent waypoint when the target node moves", () => {
    // Target node dragged to (260, 140). The last segment w1->target was
    // horizontal, so w1 must follow the target's new y to stay horizontal.
    const source: Point = { x: 0, y: 0 };
    const target: Point = { x: 260, y: 140 };

    const out = reanchorWaypoints(source, target, waypoints);

    expectOrthogonal(source, out, target);
    // w1 tracked the target on the shared (y) axis; its x is untouched.
    expect(out[1]).toEqual({ x: 100, y: 140 });
  });

  it("re-anchors a single-waypoint L-bend, preserving the elbow shape", () => {
    // A VH elbow pinned with source(0,0), target(200,80):
    //   source(0,0) -> w0(0,80) -> target(200,80)
    // source->w0 vertical (share x with source), w0->target horizontal (share y
    // with target). After the source moves to (40,-20) the elbow must keep that
    // VH shape: w0.x tracks the source, w0.y tracks the target.
    const elbow: Point[] = [{ x: 0, y: 80 }];
    const source: Point = { x: 40, y: -20 };
    const target: Point = { x: 200, y: 80 };

    const out = reanchorWaypoints(source, target, elbow);

    expectOrthogonal(source, out, target);
    expect(out[0]).toEqual({ x: 40, y: 80 });
  });

  it("re-anchors both ends when both endpoints move (multi-select drag)", () => {
    const source: Point = { x: -10, y: 25 };
    const target: Point = { x: 240, y: 130 };

    const out = reanchorWaypoints(source, target, waypoints);

    expectOrthogonal(source, out, target);
    expect(out[0]).toEqual({ x: 100, y: 25 });
    expect(out[1]).toEqual({ x: 100, y: 130 });
  });

  it("leaves interior waypoints untouched (only endpoint-adjacent ones move)", () => {
    // Three waypoints: w1 is purely interior and must not be re-anchored.
    const threeWp: Point[] = [
      { x: 50, y: 0 }, // w0 (source-adjacent)
      { x: 50, y: 40 }, // w1 (interior)
      { x: 150, y: 40 }, // w2 (target-adjacent)
    ];
    // Source->w0 was horizontal (share y=0); w2->target was vertical (share
    // x=150). Move source's y and target's x.
    const source: Point = { x: 0, y: 12 };
    const target: Point = { x: 150, y: 90 };

    const out = reanchorWaypoints(source, target, threeWp);

    expectOrthogonal(source, out, target);
    // Interior waypoint unchanged.
    expect(out[1]).toEqual({ x: 50, y: 40 });
  });
});
