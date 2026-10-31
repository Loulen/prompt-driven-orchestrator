import { describe, expect, it } from "vitest";
import { autoEdgeRoute } from "./edgeAutoRoute";
import type { Point, Rect } from "./orthogonalRouter";

function crosses(points: Point[], rect: Rect): boolean {
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const overlapsX = Math.max(a.x, b.x) > rect.x + 0.5 && Math.min(a.x, b.x) < rect.x + rect.width - 0.5;
    const overlapsY = Math.max(a.y, b.y) > rect.y + 0.5 && Math.min(a.y, b.y) < rect.y + rect.height - 0.5;
    if (overlapsX && overlapsY) return true;
  }
  return false;
}

const orthogonal = (points: Point[]) =>
  points.every((p, i) => i === 0 || p.x === points[i - 1].x || p.y === points[i - 1].y);

describe("autoEdgeRoute: the lattice-snapped auto route of an edge", () => {
  // The fan-out of a Run (Start, a triage node, three cards in a row, End), as
  // laid out by a pipeline with no anchors: every wire leaves by the right.
  const triage = { x: 260, y: 100, width: 208, height: 65 };
  const left = { x: 40, y: 240, width: 208, height: 87 };
  const middle = { x: 300, y: 240, width: 208, height: 87 };
  const right = { x: 560, y: 240, width: 208, height: 65 };
  const start = { x: 300, y: 0, width: 60, height: 24 };
  const end = { x: 330, y: 420, width: 55, height: 24 };

  it("does not run back through its source card, nor cut a card on its way", () => {
    const points = autoEdgeRoute({
      source: { x: 468, y: 133 },
      target: { x: 301, y: 283 },
      sourceSide: "right",
      targetSide: "left",
      sourceRect: triage,
      targetRect: middle,
      obstacles: [start, left, right, end],
      step: 30,
    });
    expect(orthogonal(points)).toBe(true);
    expect(crosses(points.slice(1), triage)).toBe(false);
    expect(crosses(points.slice(0, -1), middle)).toBe(false);
    for (const card of [start, left, right, end]) expect(crosses(points, card)).toBe(false);
  });

  it("keeps a straight wire between two cards in a column straight", () => {
    const top = { x: 0, y: 0, width: 208, height: 60 };
    const bottom = { x: 0, y: 120, width: 208, height: 60 };
    const points = autoEdgeRoute({
      source: { x: 104, y: 60 },
      target: { x: 104, y: 120 },
      sourceSide: "bottom",
      targetSide: "top",
      sourceRect: top,
      targetRect: bottom,
      obstacles: [],
      step: 30,
    });
    expect(points.every((p) => p.x === 104)).toBe(true);
    expect(points[0]).toEqual({ x: 104, y: 60 });
    expect(points.at(-1)).toEqual({ x: 104, y: 120 });
  });
});

describe("autoEdgeRoute: the snap never lays a wire on a card's border", () => {
  it("keeps the router's bend when the lattice would put it on a neighbour's edge", () => {
    // Left card to End, below the middle card whose left border sits on x = 300:
    // the lattice line nearest to the router's bend.
    const left = { x: 40, y: 240, width: 208, height: 87 };
    const middle = { x: 300, y: 240, width: 208, height: 87 };
    const end = { x: 330, y: 420, width: 55, height: 24 };
    const points = autoEdgeRoute({
      source: { x: 248, y: 284 },
      target: { x: 331, y: 432 },
      sourceSide: "right",
      targetSide: "left",
      sourceRect: left,
      targetRect: end,
      obstacles: [middle],
      step: 30,
    });
    const onBorder = points.some((p, i) => i > 0 && p.x === 300 && points[i - 1].x === 300);
    expect(onBorder).toBe(false);
    expect(orthogonal(points)).toBe(true);
  });
});
