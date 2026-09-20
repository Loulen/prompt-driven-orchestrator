import { describe, it, expect } from "vitest";
import {
  ALL_NODE_KINDS,
  DEFAULT_PERFORMANCE_BAND,
  DURATION_MODES,
  TAB_COHORT_DEFAULTS,
  performanceBandDeviates,
  type PerformanceBand,
} from "./statsFilters";

describe("statsFilters — what Stats opens on (#819)", () => {
  it("opens Performance narrowed and the other tabs on every run", () => {
    // Durations must not be polluted by runs still in flight; spend and errors
    // must not be hidden.
    expect(TAB_COHORT_DEFAULTS).toEqual({
      overview: false,
      cost: false,
      performance: true,
    });
  });

  it("opens Performance on Active, every kind, Full whiskers, independent scales", () => {
    expect(DEFAULT_PERFORMANCE_BAND).toEqual({
      durationMode: "active",
      nodeKinds: [...ALL_NODE_KINDS],
      zoom: "full",
      axis: "independent",
    });
    // Total first, then what it splits into — the order the control walks.
    expect(DURATION_MODES).toEqual(["total", "active", "waiting"]);
  });

  it("reads the defaults as no deviation at all", () => {
    expect(
      performanceBandDeviates(
        TAB_COHORT_DEFAULTS.performance,
        DEFAULT_PERFORMANCE_BAND,
      ),
    ).toBe(false);
  });

  it("calls out a deviation on any of the five controls", () => {
    const deviating: [boolean, PerformanceBand][] = [
      [false, DEFAULT_PERFORMANCE_BAND],
      [true, { ...DEFAULT_PERFORMANCE_BAND, durationMode: "total" }],
      [true, { ...DEFAULT_PERFORMANCE_BAND, nodeKinds: ["standard"] }],
      [true, { ...DEFAULT_PERFORMANCE_BAND, zoom: "box" }],
      [true, { ...DEFAULT_PERFORMANCE_BAND, axis: "shared" }],
    ];
    for (const [completedOnly, band] of deviating) {
      expect(performanceBandDeviates(completedOnly, band)).toBe(true);
    }
  });
});
