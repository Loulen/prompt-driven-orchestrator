import { describe, it, expect } from "vitest";
import { drawnEdgeLayout } from "./drawnEdge";
import { WIRING_GRID_STEP } from "./wiringGrid";
import { anchorPoint, enforcePerpendicularEnds, landingLeg } from "./anchorSide";
import type { Point } from "./orthogonalRouter";
import {
  advanceGesture,
  droppedPath,
  gesturePath,
  landingAt,
  startGesture,
  type HoveredHandle,
  type HoveredNode,
} from "./wiringGesture";

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

  it("never saves a route that runs through the target card (#844, FP finding 2)", () => {
    // The whole chain, as the canvas runs it: a gesture that dives at the BOTTOM
    // of an End marker whose `result` handle is declared on its TOP border. The
    // preview is pinned to that handle — the drop position has no say — so the
    // route persisted is the one that will be rendered, and it goes round the
    // card rather than through it. Before the fix the preview landed on the
    // bottom, and its approach to that phantom border was written to the file as
    // waypoints inside the node.
    const pin = { side: "top" as const, point: anchorPoint(TGT_RECT, null, "top") };
    let gesture = startGesture(SRC, "bottom", WIRING_GRID_STEP);
    for (const cursor of [
      { x: 80, y: 240 },
      { x: 500, y: 245 },
      { x: 500, y: 470 }, // over the card: the trace freezes
    ]) {
      gesture = advanceGesture(gesture, {
        cursor,
        shift: false,
        overTarget: cursor.y > TGT_RECT.y,
        step: WIRING_GRID_STEP,
      });
    }
    const traced = gesturePath(
      gesture,
      landingAt({ x: 500, y: 470 }, TGT_RECT, pin),
      WIRING_GRID_STEP,
    );

    const layout = drawnEdgeLayout({
      traced,
      sourceAnchor: SOURCE_ANCHOR,
      targetAnchor: null,
      targetSide: "top",
      anchorsByDrop: false,
    });
    // No waypoint inside the card…
    for (const w of layout.waypoints ?? []) {
      const inside =
        w.x > TGT_RECT.x &&
        w.x < TGT_RECT.x + TGT_RECT.width &&
        w.y > TGT_RECT.y &&
        w.y < TGT_RECT.y + TGT_RECT.height;
      expect({ ...w, inside }).toEqual({ ...w, inside: false });
    }
    // …and, the stronger property, no SEGMENT of the reloaded route through it
    // either: the spur the FP caught had both its ends outside the card and its
    // body straight across it.
    const leg = landingLeg(WIRING_GRID_STEP);
    const reloaded = enforcePerpendicularEnds(
      [SRC, ...(layout.waypoints ?? []), pin.point],
      "bottom",
      "top",
      leg,
    );
    for (let i = 1; i < reloaded.length - 1; i++) {
      const [a, b] = [reloaded[i - 1], reloaded[i]];
      const spans = (lo: number, hi: number, e0: number, e1: number) =>
        Math.min(lo, hi) < e1 && Math.max(lo, hi) > e0;
      expect({
        segment: [a, b],
        through:
          spans(a.x, b.x, TGT_RECT.x, TGT_RECT.x + TGT_RECT.width) &&
          spans(a.y, b.y, TGT_RECT.y, TGT_RECT.y + TGT_RECT.height),
      }).toEqual({ segment: [a, b], through: false });
    }
    // …and what is reloaded is what was drawn.
    expect(reloaded).toEqual(enforcePerpendicularEnds(traced, "bottom", "top", leg));
  });

  describe("a pinned target approached from its far side, dropped on the next event (#844 FP iter-2)", () => {
    // End marker at TGT_RECT, its declared `result` handle covering the card and
    // pinned on the TOP border. The gesture comes from BELOW.
    const END_NODE: HoveredNode = {
      id: "end",
      measured: { width: TGT_RECT.width, height: TGT_RECT.height },
      internals: { positionAbsolute: { x: TGT_RECT.x, y: TGT_RECT.y } },
      data: { nodeType: "end" },
    };
    const END_HANDLE: HoveredHandle = {
      x: TGT_RECT.x + TGT_RECT.width / 2,
      y: TGT_RECT.y + TGT_RECT.height / 2,
      width: TGT_RECT.width,
      height: TGT_RECT.height,
      position: "top",
    };
    const PIN = { x: 500, y: 400 };
    const DROP = { x: 500, y: 470 };

    /** The gesture as the connection line leaves it: xyflow refreshes its hover
     *  on `mousemove`, AFTER our `pointermove`, so the frame that entered the card
     *  still believed nothing was hovered. */
    function staleGesture() {
      let gesture = startGesture(SRC, "bottom", WIRING_GRID_STEP);
      for (const cursor of [{ x: 80, y: 560 }, { x: 500, y: 560 }, DROP]) {
        gesture = advanceGesture(gesture, {
          cursor,
          shift: false,
          overTarget: false,
          step: WIRING_GRID_STEP,
        });
      }
      return gesture;
    }

    function reloadedFrom(traced: Point[]): Point[] {
      const layout = drawnEdgeLayout({
        traced,
        sourceAnchor: SOURCE_ANCHOR,
        targetAnchor: null,
        targetSide: "top",
        anchorsByDrop: false,
      });
      return enforcePerpendicularEnds(
        [SRC, ...(layout.waypoints ?? []), PIN],
        "bottom",
        "top",
        landingLeg(WIRING_GRID_STEP),
      );
    }

    function crossesCard(route: Point[]): boolean {
      const spans = (lo: number, hi: number, e0: number, e1: number) =>
        Math.min(lo, hi) < e1 && Math.max(lo, hi) > e0;
      for (let i = 1; i < route.length; i++) {
        const [a, b] = [route[i - 1], route[i]];
        if (
          spans(a.x, b.x, TGT_RECT.x, TGT_RECT.x + TGT_RECT.width) &&
          spans(a.y, b.y, TGT_RECT.y, TGT_RECT.y + TGT_RECT.height)
        )
          return true;
      }
      return false;
    }

    it("reproduces the defect with the trace the last pointer move published", () => {
      // What `onConnectEnd` used to persist: no landing at all, so the renderer
      // squares the gap straight up through the card.
      expect(crossesCard(reloadedFrom(gesturePath(staleGesture(), null, WIRING_GRID_STEP)))).toBe(true);
    });

    it("goes around the card once the landing is rebuilt from the drop's own state", () => {
      const traced = droppedPath(staleGesture(), DROP, END_NODE, END_HANDLE, "src", WIRING_GRID_STEP);
      expect(traced[traced.length - 1]).toEqual(PIN);
      const reloaded = reloadedFrom(traced);
      expect(crossesCard(reloaded)).toBe(false);
      // …and what is reloaded is what the preview drew.
      expect(reloaded).toEqual(
        enforcePerpendicularEnds(traced, "bottom", "top", landingLeg(WIRING_GRID_STEP)),
      );
    });

    it("keeps the published trace when nothing is hovered at the drop", () => {
      const gesture = staleGesture();
      expect(droppedPath(gesture, DROP, null, null, "src", WIRING_GRID_STEP)).toEqual(
        gesturePath(gesture, null, WIRING_GRID_STEP),
      );
    });
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
