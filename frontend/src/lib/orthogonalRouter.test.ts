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
