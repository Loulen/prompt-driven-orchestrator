import { describe, it, expect } from "vitest";
import {
  conditionLabelOffset,
  firstSegmentHeading,
  midpointAxis,
  outputLabelPlacement,
  outputLabelsDefault,
  resolveOutputLabels,
  segmentAxis,
} from "./edgeLabels";

describe("outputLabelPlacement (#845)", () => {
  it("alternates around the base of a rightward arrow, 1st above and 2nd below", () => {
    const first = outputLabelPlacement(0, "horizontal", 1);
    const second = outputLabelPlacement(1, "horizontal", 1);
    expect(first.y).toBeLessThan(0); // above the stroke
    expect(second.y).toBeGreaterThan(0); // below it
    // Both sit the same distance clear of the stroke, and both start just past
    // the card's rim so a long port name grows along the wire, not under the card.
    expect(Math.abs(first.y)).toBe(Math.abs(second.y));
    expect(first.x).toBeGreaterThan(0);
    expect(first.align).toBe("start");
  });

  it("walks outwards in ranks: the 3rd goes beyond the 1st, the 4th beyond the 2nd", () => {
    const [a, b, c, d] = [0, 1, 2, 3].map((i) => outputLabelPlacement(i, "horizontal", 1));
    expect(Math.abs(c.y)).toBeGreaterThan(Math.abs(a.y));
    expect(Math.abs(d.y)).toBeGreaterThan(Math.abs(b.y));
    // …and stays on its own side of the stroke.
    expect(Math.sign(c.y)).toBe(Math.sign(a.y));
    expect(Math.sign(d.y)).toBe(Math.sign(b.y));
  });

  it("grows leftwards from the rim when the arrow leaves to the left", () => {
    const left = outputLabelPlacement(0, "horizontal", -1);
    expect(left.x).toBeLessThan(0);
    expect(left.align).toBe("end");
  });

  it("swaps the axes when the arrow leaves vertically — labels left and right of the stroke", () => {
    const first = outputLabelPlacement(0, "vertical", 1);
    const second = outputLabelPlacement(1, "vertical", 1);
    expect(first.x).toBeLessThan(0);
    expect(second.x).toBeGreaterThan(0);
    // Both sit DOWN the wire (dir = +1), clear of the card.
    expect(first.y).toBeGreaterThan(0);
    expect(second.y).toBe(first.y);
    expect(first.align).toBe("end");
    expect(second.align).toBe("start");
  });

  it("sends the labels up the wire when the arrow leaves upwards", () => {
    expect(outputLabelPlacement(0, "vertical", -1).y).toBeLessThan(0);
  });
});

describe("outputLabelsDefault / resolveOutputLabels (#845)", () => {
  it("shows the names only when the source declares two or more outputs", () => {
    expect(outputLabelsDefault(0)).toBe(false);
    expect(outputLabelsDefault(1)).toBe(false);
    expect(outputLabelsDefault(2)).toBe(true);
    expect(outputLabelsDefault(5)).toBe(true);
  });

  it("falls back to the count only while the edge has no explicit toggle", () => {
    expect(resolveOutputLabels(undefined, 2)).toBe(true);
    expect(resolveOutputLabels(null, 1)).toBe(false);
  });

  it("lets an explicit toggle win, both ways", () => {
    // The `false` case is the one that matters: it is what "I turned them off"
    // means on a node that declares two outputs.
    expect(resolveOutputLabels(false, 3)).toBe(false);
    expect(resolveOutputLabels(true, 1)).toBe(true);
  });
});

describe("conditionLabelOffset (#845)", () => {
  it("nudges the pill off the stroke, perpendicular to the segment it sits on", () => {
    // Straddling the stroke put the pill exactly on the midpoint segment handle,
    // which then could not be grabbed at all.
    const onHorizontal = conditionLabelOffset("horizontal");
    expect(onHorizontal.x).toBe(0);
    expect(onHorizontal.y).not.toBe(0);

    const onVertical = conditionLabelOffset("vertical");
    expect(onVertical.y).toBe(0);
    expect(onVertical.x).not.toBe(0);
  });
});

describe("segmentAxis / firstSegmentHeading", () => {
  it("reads a segment's orientation, and nothing from a degenerate one", () => {
    expect(segmentAxis({ x: 0, y: 0 }, { x: 10, y: 0 })).toBe("horizontal");
    expect(segmentAxis({ x: 0, y: 0 }, { x: 0, y: 10 })).toBe("vertical");
    expect(segmentAxis({ x: 5, y: 5 }, { x: 5, y: 5 })).toBeNull();
  });

  it("reports the first segment's axis and direction", () => {
    expect(firstSegmentHeading([{ x: 0, y: 0 }, { x: 40, y: 0 }])).toEqual({
      axis: "horizontal",
      dir: 1,
    });
    expect(firstSegmentHeading([{ x: 40, y: 0 }, { x: 0, y: 0 }])).toEqual({
      axis: "horizontal",
      dir: -1,
    });
    expect(firstSegmentHeading([{ x: 0, y: 0 }, { x: 0, y: -40 }])).toEqual({
      axis: "vertical",
      dir: -1,
    });
  });

  it("reads a degenerate path as leaving rightwards", () => {
    expect(firstSegmentHeading([{ x: 7, y: 7 }])).toEqual({ axis: "horizontal", dir: 1 });
    expect(firstSegmentHeading([])).toEqual({ axis: "horizontal", dir: 1 });
  });
});

describe("midpointAxis", () => {
  const path = [
    { x: 0, y: 0 },
    { x: 100, y: 0 },
    { x: 100, y: 100 },
  ];

  it("reports the orientation of the segment the midpoint falls on", () => {
    expect(midpointAxis(path, { x: 50, y: 0 })).toBe("horizontal");
    expect(midpointAxis(path, { x: 100, y: 50 })).toBe("vertical");
  });

  it("falls back to horizontal for a point off the path", () => {
    expect(midpointAxis(path, { x: -500, y: -500 })).toBe("horizontal");
  });
});
