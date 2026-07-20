import { describe, it, expect } from "vitest";
import {
  costPrecision,
  formatEstCost,
  formatBucketCost,
} from "./costLabel";

describe("costPrecision", () => {
  it("uses 4 decimals below $1 and 2 at or above", () => {
    expect(costPrecision(0.0525)).toBe(4);
    expect(costPrecision(0.999)).toBe(4);
    expect(costPrecision(1)).toBe(2);
    expect(costPrecision(12.5)).toBe(2);
  });
});

describe("formatEstCost (single run, #272)", () => {
  it("renders ~$ at 2 decimals for >= $1, no dagger, estimate tooltip", () => {
    const c = formatEstCost(1.23, false);
    expect(c.text).toBe("~$1.23");
    expect(c.dagger).toBe(false);
    expect(c.title).toMatch(/estimate/i);
    expect(c.title).not.toMatch(/lower bound/i);
  });

  it("renders 4 decimals for a sub-dollar estimate", () => {
    expect(formatEstCost(0.0525, false).text).toBe("~$0.0525");
  });

  it("flags a partial estimate with a dagger and a lower-bound tooltip", () => {
    const c = formatEstCost(2.5, true);
    expect(c.text).toBe("~$2.50");
    expect(c.dagger).toBe(true);
    expect(c.title).toMatch(/lower bound/i);
  });
});

describe("formatBucketCost (aggregate, #377)", () => {
  it("sums a plain bucket with no partial/null contributions", () => {
    const c = formatBucketCost(3.4, 0, 0, 2);
    expect(c.text).toBe("~$3.40");
    expect(c.dagger).toBe(false);
    expect(c.empty).toBe(false);
    expect(c.title).toMatch(/estimate/i);
    expect(c.title).not.toMatch(/lower bound/i);
    expect(c.title).not.toMatch(/no transcript/i);
  });

  it("marks a bucket with a partial run as a lower bound and counts it", () => {
    const c = formatBucketCost(5.0, 1, 0, 3);
    expect(c.dagger).toBe(true);
    expect(c.text).toBe("~$5.00");
    expect(c.title).toMatch(/lower bound/i);
    expect(c.title).toMatch(/1 partial run\b/);
  });

  it("pluralises partial run count", () => {
    expect(formatBucketCost(5.0, 2, 0, 4).title).toMatch(/2 partial runs/);
  });

  it("surfaces null-cost runs in the tooltip without inflating the figure", () => {
    const c = formatBucketCost(2.0, 0, 1, 3);
    expect(c.text).toBe("~$2.00");
    expect(c.empty).toBe(false);
    expect(c.title).toMatch(/1 run had no transcript \(excluded\)/);
  });

  it("renders — (never $0) for a bucket with no priced runs (all null)", () => {
    const c = formatBucketCost(0, 0, 3, 3);
    expect(c.text).toBe("—");
    expect(c.empty).toBe(true);
    expect(c.text).not.toContain("$");
    // The tooltip still explains why it is empty.
    expect(c.title).toMatch(/3 runs had no transcript/);
  });

  it("renders — for a bucket with no runs at all", () => {
    const c = formatBucketCost(0, 0, 0, 0);
    expect(c.text).toBe("—");
    expect(c.empty).toBe(true);
  });
});
