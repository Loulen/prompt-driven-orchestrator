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

describe("autoEdgeRoute: a wire leaving a loop skips the cards beyond it", () => {
  // A column (implementer, tester, Ship It) with End below, the first two in a
  // loop: the tester's conditional output skips Ship It and lands on End.
  const implementer = { x: 320, y: 173, width: 208, height: 65 };
  const tester = { x: 327, y: 265, width: 208, height: 65 };
  const shipIt = { x: 327, y: 385, width: 208, height: 65 };
  const end = { x: 330, y: 476, width: 55, height: 24 };
  const loop = { x: 296, y: 140, width: 264, height: 215, region: true };

  for (const step of [20, 30, 40]) {
    it(`goes around the card in between, not through it (step ${step})`, () => {
      const points = autoEdgeRoute({
        source: { x: 431, y: 330 },
        target: { x: 330, y: 488 },
        sourceSide: "bottom",
        targetSide: "left",
        sourceRect: tester,
        targetRect: end,
        obstacles: [implementer, shipIt, loop],
        step,
      });
      expect(orthogonal(points)).toBe(true);
      expect(crosses(points, shipIt)).toBe(false);
      expect(crosses(points, implementer)).toBe(false);
      expect(crosses(points.slice(1), tester)).toBe(false);
      expect(crosses(points.slice(0, -1), end)).toBe(false);
    });

    it(`never doubles back on itself (step ${step})`, () => {
      const points = autoEdgeRoute({
        source: { x: 431, y: 330 },
        target: { x: 330, y: 488 },
        sourceSide: "bottom",
        targetSide: "left",
        sourceRect: tester,
        targetRect: end,
        obstacles: [implementer, shipIt, loop],
        step,
      });
      const reverses = points.some((b, i) => {
        if (i === 0 || i === points.length - 1) return false;
        const [a, c] = [points[i - 1], points[i + 1]];
        if (a.y === b.y && b.y === c.y) return (b.x - a.x) * (c.x - b.x) < 0;
        if (a.x === b.x && b.x === c.x) return (b.y - a.y) * (c.y - b.y) < 0;
        return false;
      });
      expect(reverses).toBe(false);
    });
  }
});
