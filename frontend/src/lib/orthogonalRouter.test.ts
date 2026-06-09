import { describe, it, expect } from "vitest";
import { routeOrthogonal, type Point, type Rect } from "./orthogonalRouter";

// True if any segment of the polyline passes through the interior of `rect`.
function pathCrosses(points: Point[], rect: Rect): boolean {
  const x0 = rect.x;
  const x1 = rect.x + rect.width;
  const y0 = rect.y;
  const y1 = rect.y + rect.height;
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const segMinX = Math.min(a.x, b.x);
    const segMaxX = Math.max(a.x, b.x);
    const segMinY = Math.min(a.y, b.y);
    const segMaxY = Math.max(a.y, b.y);
    // Overlap of the segment's bounding box with the rect's open interior.
    const overlapsX = segMaxX > x0 && segMinX < x1;
    const overlapsY = segMaxY > y0 && segMinY < y1;
    if (overlapsX && overlapsY) return true;
  }
  return false;
}

// All segments of a polyline are axis-aligned (purely horizontal or vertical).
function isOrthogonal(points: Point[]): boolean {
  for (let i = 1; i < points.length; i++) {
    const dx = Math.abs(points[i].x - points[i - 1].x);
    const dy = Math.abs(points[i].y - points[i - 1].y);
    if (dx > 1e-6 && dy > 1e-6) return false;
  }
  return true;
}

describe("routeOrthogonal", () => {
  it("connects two aligned endpoints with a straight orthogonal run", () => {
    const source: Point = { x: 0, y: 100 };
    const target: Point = { x: 200, y: 100 };
    const path = routeOrthogonal({ source, target, obstacles: [] });

    expect(path[0]).toEqual(source);
    expect(path[path.length - 1]).toEqual(target);
    expect(isOrthogonal(path)).toBe(true);
  });

  it("inserts right-angle bends for offset endpoints (never a diagonal)", () => {
    const source: Point = { x: 0, y: 0 };
    const target: Point = { x: 200, y: 120 };
    const path = routeOrthogonal({ source, target, obstacles: [] });

    expect(path[0]).toEqual(source);
    expect(path[path.length - 1]).toEqual(target);
    expect(path.length).toBeGreaterThanOrEqual(3);
    expect(isOrthogonal(path)).toBe(true);
  });

  it("detours around an obstacle sitting on the straight line", () => {
    const source: Point = { x: 0, y: 100 };
    const target: Point = { x: 300, y: 100 };
    // A node squarely between the two endpoints, straddling y=100.
    const obstacle: Rect = { x: 120, y: 60, width: 80, height: 80 };

    const path = routeOrthogonal({ source, target, obstacles: [obstacle] });

    expect(path[0]).toEqual(source);
    expect(path[path.length - 1]).toEqual(target);
    expect(isOrthogonal(path)).toBe(true);
    expect(pathCrosses(path, obstacle)).toBe(false);
  });

  it("is deterministic and re-routes when an obstacle moves", () => {
    const source: Point = { x: 0, y: 100 };
    const target: Point = { x: 300, y: 100 };
    const obstacle: Rect = { x: 120, y: 60, width: 80, height: 80 };

    const a = routeOrthogonal({ source, target, obstacles: [obstacle] });
    const b = routeOrthogonal({ source, target, obstacles: [obstacle] });
    // Same inputs → same path (pure, recomputed deterministically per AC).
    expect(b).toEqual(a);

    // Move the obstacle clear of the straight line: the route collapses back to
    // the direct run, proving it re-routes on a node move.
    const moved: Rect = { x: 120, y: 300, width: 80, height: 80 };
    const c = routeOrthogonal({ source, target, obstacles: [moved] });
    expect(c).not.toEqual(a);
    expect(c).toEqual([source, target]);
    expect(pathCrosses(c, moved)).toBe(false);
  });
});

describe("routeOrthogonal — arrives from the anchored target side (#175)", () => {
  // The segment just before the target tells us which side the arrow enters.
  const target: Point = { x: 200, y: 100 };

  it("defaults to a left arrival (legacy left->right) when no side is given", () => {
    // Source to the left, vertically offset: the final segment runs in from the
    // left (approach x < target.x, level with the target row).
    const path = routeOrthogonal({ source: { x: 0, y: 0 }, target, obstacles: [] });
    const approach = path[path.length - 2];
    expect(isOrthogonal(path)).toBe(true);
    expect(path[path.length - 1]).toEqual(target);
    expect(approach.x).toBeLessThan(target.x);
    expect(approach.y).toBeCloseTo(target.y, 6);
  });

  it("arrives horizontally from the right when target_side is right", () => {
    // Natural geometry: source sits to the right of the target.
    const path = routeOrthogonal({
      source: { x: 400, y: 100 },
      target,
      obstacles: [],
      targetSide: "right",
    });
    const approach = path[path.length - 2];
    expect(isOrthogonal(path)).toBe(true);
    expect(path[path.length - 1]).toEqual(target);
    expect(approach.x).toBeGreaterThan(target.x);
    expect(approach.y).toBeCloseTo(target.y, 6);
  });

  it("arrives vertically from above when target_side is top", () => {
    // Natural geometry: source sits above the target.
    const path = routeOrthogonal({
      source: { x: 200, y: -100 },
      target,
      obstacles: [],
      targetSide: "top",
    });
    const approach = path[path.length - 2];
    expect(isOrthogonal(path)).toBe(true);
    expect(path[path.length - 1]).toEqual(target);
    expect(approach.y).toBeLessThan(target.y);
    expect(approach.x).toBeCloseTo(target.x, 6);
  });

  it("arrives vertically from below when target_side is bottom", () => {
    // Natural geometry: source sits below the target.
    const path = routeOrthogonal({
      source: { x: 200, y: 400 },
      target,
      obstacles: [],
      targetSide: "bottom",
    });
    const approach = path[path.length - 2];
    expect(isOrthogonal(path)).toBe(true);
    expect(path[path.length - 1]).toEqual(target);
    expect(approach.y).toBeGreaterThan(target.y);
    expect(approach.x).toBeCloseTo(target.x, 6);
  });

  it("still enters from the right even when the source is to the left (turns in from outside)", () => {
    // The hard case the legacy HVH got wrong: source left of target, but the
    // arrow must still land on the right edge. The approach must come from
    // x > target.x — never straight across the body from the left.
    const path = routeOrthogonal({
      source: { x: 0, y: 0 },
      target,
      obstacles: [],
      targetSide: "right",
    });
    const approach = path[path.length - 2];
    expect(isOrthogonal(path)).toBe(true);
    expect(path[path.length - 1]).toEqual(target);
    expect(approach.x).toBeGreaterThan(target.x);
    expect(approach.y).toBeCloseTo(target.y, 6);
  });

  it("is deterministic for a given anchored side", () => {
    const input = {
      source: { x: 0, y: 0 },
      target,
      obstacles: [] as Rect[],
      targetSide: "top" as const,
    };
    expect(routeOrthogonal(input)).toEqual(routeOrthogonal(input));
  });
});
