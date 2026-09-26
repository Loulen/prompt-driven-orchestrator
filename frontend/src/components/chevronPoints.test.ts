import { describe, it, expect } from "vitest";
import { chevronPoints } from "./chevronPoints";
import type { PortSide } from "../shared/generated/PortSide";

const SIDES: PortSide[] = ["left", "right", "top", "bottom"];
const KINDS = ["input", "output"] as const;

describe("chevronPoints", () => {
  it("returns a truthy string for all 8 (kind, side) combinations", () => {
    for (const kind of KINDS) {
      for (const side of SIDES) {
        const pts = chevronPoints(kind, side);
        expect(pts).toBeTruthy();
      }
    }
  });

  it("input left chevron points inward (rightward)", () => {
    expect(chevronPoints("input", "left")).toBe("2,2 6,5 2,8");
  });

  it("output left chevron points outward (leftward)", () => {
    expect(chevronPoints("output", "left")).toBe("6,2 2,5 6,8");
  });

  it("input right chevron points inward (leftward)", () => {
    expect(chevronPoints("input", "right")).toBe("6,2 2,5 6,8");
  });

  it("output right chevron points outward (rightward)", () => {
    expect(chevronPoints("output", "right")).toBe("2,2 6,5 2,8");
  });

  it("input top chevron points inward (downward)", () => {
    expect(chevronPoints("input", "top")).toBe("2,2 5,6 8,2");
  });

  it("output top chevron points outward (upward)", () => {
    expect(chevronPoints("output", "top")).toBe("2,6 5,2 8,6");
  });

  it("input bottom chevron points inward (upward)", () => {
    expect(chevronPoints("input", "bottom")).toBe("2,6 5,2 8,6");
  });

  it("output bottom chevron points outward (downward)", () => {
    expect(chevronPoints("output", "bottom")).toBe("2,2 5,6 8,2");
  });

  it("input and output on same side produce different points", () => {
    for (const side of SIDES) {
      const inPts = chevronPoints("input", side);
      const outPts = chevronPoints("output", side);
      expect(inPts).not.toBe(outPts);
    }
  });
});
