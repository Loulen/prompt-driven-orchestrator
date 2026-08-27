import { describe, it, expect } from "vitest";
import { costPrecision, formatCostAmount, formatEstCost } from "./costLabel";

describe("costPrecision", () => {
  it("uses 4 decimals below $1 and 2 at or above", () => {
    expect(costPrecision(0.0525)).toBe(4);
    expect(costPrecision(0.999)).toBe(4);
    expect(costPrecision(1)).toBe(2);
    expect(costPrecision(12.5)).toBe(2);
  });

  describe("formatCostAmount (#638)", () => {
    it("distinguishes derived estimates from reported cost", () => {
      expect(formatCostAmount(2, false, true)).toBe("~$2.00");
      expect(formatCostAmount(2, false, false)).toBe("$2.00");
      expect(formatCostAmount(null, false, false)).toBe("—");
    });
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

  it("names the excluded model(s) in the tooltip when known (#425)", () => {
    const one = formatEstCost(2.5, true, ["claude-sonnet-5"]);
    expect(one.dagger).toBe(true);
    expect(one.title).toMatch(/lower bound/i);
    expect(one.title).toContain("claude-sonnet-5");
    expect(one.title).toMatch(/unpriced model excluded/);

    const many = formatEstCost(2.5, true, ["claude-fable-5", "claude-sonnet-5"]);
    expect(many.title).toContain("claude-fable-5");
    expect(many.title).toContain("claude-sonnet-5");
    expect(many.title).toMatch(/unpriced models excluded/);
  });

  it("falls back to the generic note when partial but no names are given", () => {
    const c = formatEstCost(2.5, true, []);
    expect(c.title).toMatch(/an unpriced model was excluded/);
  });

  it("renders — (never $0, no dagger) and names the harness for an uncosted harness (#553)", () => {
    // A Run with a node on a harness with no cost source: "—" with a reason
    // naming the harness — categorically different from a lower bound.
    const c = formatEstCost(0, false, [], ["opencode"]);
    expect(c.text).toBe("—");
    expect(c.text).not.toContain("$");
    expect(c.dagger).toBe(false);
    expect(c.title).toMatch(/cost unavailable/i);
    expect(c.title).toContain("opencode");
    expect(c.title).toMatch(/no cost source/);
  });

  it("takes the uncosted branch even when partial/priced data is present (#553)", () => {
    // "Unavailable" is a stronger statement than "lower bound": it wins.
    const c = formatEstCost(4.2, true, ["claude-fable-5"], ["opencode", "codex"]);
    expect(c.text).toBe("—");
    expect(c.dagger).toBe(false);
    expect(c.title).toContain("opencode");
    expect(c.title).toContain("codex");
    expect(c.title).toMatch(/harnesses .* have no cost source/);
    // Not framed as a lower bound — there is no figure at all.
    expect(c.title).not.toContain("claude-fable-5");
  });
});
