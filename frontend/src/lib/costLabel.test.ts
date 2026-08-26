import { describe, it, expect } from "vitest";
import {
  costPrecision,
  formatEstCost,
  formatBucketCost,
  COST_ESTIMATE_NOTE,
  COST_REPORTED_NOTE,
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

  it("ventilates a mixed Run by harness, summable but said per harness (#615)", () => {
    const c = formatEstCost(7.0, false, [], [], [
      { harness: "claude", usd: 5.0, form: "derived", partial: false, unpriced_models: [] },
      { harness: "copilot", usd: 2.0, form: "reported", partial: false, unpriced_models: [] },
    ]);
    // The total stays summable.
    expect(c.text).toBe("~$7.00");
    // Breakdown carries both slices, each with its form.
    expect(c.ventilation).toEqual([
      { harness: "claude", text: "~$5.00", form: "derived" },
      { harness: "copilot", text: "~$2.00", form: "reported" },
    ]);
    // The Claude-Code estimate wording appears only under the derived slice…
    expect(c.title).toMatch(/via `claude` \(derived\)/);
    expect(c.title).toContain(COST_ESTIMATE_NOTE);
    // …and the reported slice is framed as reported, not as an estimate.
    expect(c.title).toMatch(/via `copilot` \(reported\)/);
    expect(c.title).toContain(COST_REPORTED_NOTE);
  });

  it("does not label a pure-copilot Run as a Claude-Code estimate (#615)", () => {
    const c = formatEstCost(1.0, false, [], [], [
      { harness: "copilot", usd: 1.0, form: "reported", partial: false, unpriced_models: [] },
    ]);
    expect(c.text).toBe("~$1.00");
    // The AC: "estimate from Claude Code transcripts" shows ONLY under a derived
    // cost — a reported copilot figure must not carry it.
    expect(c.title).not.toContain(COST_ESTIMATE_NOTE);
    expect(c.title).toContain(COST_REPORTED_NOTE);
    expect(c.dagger).toBe(false);
  });

  it("daggers a mixed Run only when a DERIVED slice is a lower bound (#615)", () => {
    const c = formatEstCost(7.0, true, ["claude-opus-6"], [], [
      { harness: "claude", usd: 5.0, form: "derived", partial: true, unpriced_models: ["claude-opus-6"] },
      { harness: "copilot", usd: 2.0, form: "reported", partial: false, unpriced_models: [] },
    ]);
    expect(c.dagger).toBe(true);
    expect(c.title).toContain("claude-opus-6");
    // The reported slice never contributes an unpriced-model name.
    expect(c.title).toMatch(/via `copilot` \(reported\)/);
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

  it("names the unioned unpriced models alongside the partial-run count (#425)", () => {
    const c = formatBucketCost(5.0, 2, 0, 4, ["claude-fable-5", "claude-sonnet-5"]);
    expect(c.dagger).toBe(true);
    expect(c.title).toMatch(/lower bound/i);
    expect(c.title).toContain("claude-fable-5");
    expect(c.title).toContain("claude-sonnet-5");
    expect(c.title).toMatch(/2 partial runs/);
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
