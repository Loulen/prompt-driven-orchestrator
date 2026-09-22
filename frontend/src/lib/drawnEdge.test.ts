import { describe, it, expect } from "vitest";
import { drawnEdgeLayout } from "./drawnEdge";
import { WIRING_GRID_STEP } from "./wiringGrid";
import { anchorPoint, enforcePerpendicularEnds, landingLeg } from "./anchorSide";
import type { Point } from "./orthogonalRouter";

const SRC_RECT = { x: 0, y: 0, width: 200, height: 80 };
const TGT_RECT = { x: 400, y: 400, width: 200, height: 80 };
const SOURCE_ANCHOR = { side: "bottom" as const, offset: 80 };
const TARGET_ANCHOR = { side: "top" as const, offset: 80 };
const SRC = anchorPoint(SRC_RECT, SOURCE_ANCHOR, "bottom"); // (80, 80)
const TGT = anchorPoint(TGT_RECT, TARGET_ANCHOR, "top"); // (480, 400)

/** A gesture drawn down then right on the 40px lattice, ending on the target. */
const TRACED: Point[] = [SRC, { x: 80, y: 240 }, { x: 480, y: 240 }, TGT];

function drawn(overrides: Partial<Parameters<typeof drawnEdgeLayout>[0]> = {}) {
  return drawnEdgeLayout({
    traced: TRACED,
    sourceAnchor: SOURCE_ANCHOR,
    targetAnchor: TARGET_ANCHOR,
    targetSide: "top",
    anchorsByDrop: true,
    ...overrides,
  });
}

describe("drawnEdgeLayout (#844)", () => {
  it("is born manual, carrying the waypoints the gesture drew", () => {
    const layout = drawn();
    expect(layout.mode).toBe("manual");
    expect(layout.waypoints?.length).toBeGreaterThan(0);
  });

  it("keeps the waypoints on the wiring lattice", () => {
    for (const w of drawn().waypoints ?? []) {
      expect(w.x % WIRING_GRID_STEP).toBe(0);
      expect(w.y % WIRING_GRID_STEP).toBe(0);
    }
  });

  it("stores whole pixels — a sub-pixel waypoint is a phantom bend", () => {
    const layout = drawn({
      traced: [SRC, { x: 80.4, y: 240.7 }, { x: 480.2, y: 240.7 }, TGT],
    });
    for (const w of layout.waypoints ?? []) {
      expect(Number.isInteger(w.x)).toBe(true);
      expect(Number.isInteger(w.y)).toBe(true);
    }
  });

  it("stores neither approach point — the legs are re-derived from the anchors", () => {
    const leg = landingLeg(WIRING_GRID_STEP);
    const stored = drawn().waypoints ?? [];
    expect(stored).not.toContainEqual({ x: SRC.x, y: SRC.y + leg });
    expect(stored).not.toContainEqual({ x: TGT.x, y: TGT.y - leg });
  });

  it("round-trips: re-enforcing the stored route reproduces the drawn path", () => {
    const leg = landingLeg(WIRING_GRID_STEP);
    const drawnPath = enforcePerpendicularEnds(TRACED, "bottom", "top", leg);
    const reloaded = enforcePerpendicularEnds(
      [SRC, ...(drawn().waypoints ?? []), TGT],
      "bottom",
      "top",
      leg,
    );
    expect(reloaded).toEqual(drawnPath);
  });

  it("records both anchors, so the wire leaves and lands where it was aimed", () => {
    const layout = drawn();
    expect(layout.source_anchor).toEqual(SOURCE_ANCHOR);
    expect(layout.target_anchor).toEqual(TARGET_ANCHOR);
    expect(layout.target_side).toBe("top");
  });

  it("omits target_side on a left landing — `left` is the legacy default (#168)", () => {
    const layout = drawn({
      targetSide: "left",
      targetAnchor: { side: "left", offset: 40 },
    });
    expect("target_side" in layout).toBe(false);
    expect(layout.target_anchor).toEqual({ side: "left", offset: 40 });
  });

  it("leaves a declared-port target unanchored — it keeps its fixed side", () => {
    const layout = drawn({ anchorsByDrop: false, targetAnchor: null, targetSide: "top" });
    expect("target_side" in layout).toBe(false);
    expect("target_anchor" in layout).toBe(false);
  });

  it("does NOT pin a route that is just its two legs — that is what auto draws", () => {
    const leg = landingLeg(WIRING_GRID_STEP);
    const straightDown: Point[] = [
      { x: 80, y: 80 },
      { x: 80, y: 80 + leg },
      { x: 80, y: 400 - leg },
      { x: 80, y: 400 },
    ];
    const layout = drawnEdgeLayout({
      traced: straightDown,
      sourceAnchor: SOURCE_ANCHOR,
      targetAnchor: { side: "top", offset: 80 },
      targetSide: "top",
      anchorsByDrop: true,
    });
    expect("mode" in layout).toBe(false);
    expect("waypoints" in layout).toBe(false);
    // The anchors still ride along: the wire's endpoints ARE the gesture.
    expect(layout.source_anchor).toEqual(SOURCE_ANCHOR);
  });

  it("writes nothing at all for a drop that carried no gesture and no anchor", () => {
    expect(
      drawnEdgeLayout({
        traced: [],
        sourceAnchor: null,
        targetAnchor: null,
        targetSide: "left",
        anchorsByDrop: true,
      }),
    ).toEqual({});
  });
});
