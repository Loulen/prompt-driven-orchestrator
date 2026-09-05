import { describe, it, expect } from "vitest";
import {
  costPrecision,
  formatCostAmount,
  formatEstCost,
  nodeCostTitle,
  COST_ESTIMATE_NOTE,
  COST_REPORTED_NOTE,
  COST_REPORTED_IN_USD_NOTE,
} from "./costLabel";
import type { NodeCost } from "../types";

describe("costPrecision", () => {
  it("uses 4 decimals below $1 and 2 at or above", () => {
    expect(costPrecision(0.0525)).toBe(4);
    expect(costPrecision(0.999)).toBe(4);
    expect(costPrecision(1)).toBe(2);
    expect(costPrecision(12.5)).toBe(2);
  });

  describe("nodeCostTitle (#647)", () => {
    const cost = (overrides: Partial<NodeCost> = {}): NodeCost => ({
      usd: 1,
      form: "derived",
      partial: false,
      executions: 1,
      readable_executions: 1,
      ...overrides,
    });

    it("describes derived, reported, partial, unavailable, and repeated execution costs honestly", () => {
      expect(nodeCostTitle(cost())).toContain(COST_ESTIMATE_NOTE);
      expect(nodeCostTitle(cost({ form: "reported" }))).toContain(COST_REPORTED_NOTE);
      expect(nodeCostTitle(cost({ form: "reported", reported_in_usd: true }))).toContain(
        COST_REPORTED_IN_USD_NOTE,
      );
      expect(nodeCostTitle(cost({ form: "reported", reported_in_usd: true }))).not.toContain(
        COST_REPORTED_NOTE,
      );
      expect(nodeCostTitle(cost({ form: null }))).not.toContain(COST_ESTIMATE_NOTE);
      expect(nodeCostTitle(cost({ form: null }))).not.toContain(COST_REPORTED_NOTE);
      expect(nodeCostTitle(cost({ form: null }))).toMatch(
        /derived estimates.*reported costs/i,
      );
      expect(nodeCostTitle(cost({ partial: true }))).toMatch(/lower bound/i);
      expect(
        nodeCostTitle(cost({ usd: null, unavailable_reasons: ["missing reading"] })),
      ).toContain("missing reading");
      expect(nodeCostTitle(cost({ executions: 2 }))).toContain(
        "Covers 2 executions of this node.",
      );
      expect(nodeCostTitle(cost())).not.toContain("Covers");
    });
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

  it("says its slices under an unavailable total — only the sum is refused (#617 FP)", () => {
    // The three-harness Run: claude + opencode + copilot. `opencode` withholds the
    // TOTAL (#553), but what came through the two instrumented harnesses is known
    // and must be said (ADR-0052 §3). Suppressing the breakdown made the one Run
    // built to observe ventilation the one Run that could not show any.
    const c = formatEstCost(0, false, [], ["opencode"], [
      { harness: "claude", usd: 0.342386, form: "derived", partial: false, unpriced_models: [] },
      { harness: "copilot", usd: 0.1005058, form: "reported", partial: false, unpriced_models: [] },
    ]);

    // Still "—", still no dagger, still naming the reason.
    expect(c.text).toBe("—");
    expect(c.dagger).toBe(false);
    expect(c.title).toMatch(/cost unavailable/i);
    expect(c.title).toContain("opencode");

    // …and the two slices are there, each framed by its own form.
    expect(c.ventilation).toEqual([
      { harness: "claude", text: "~$0.3424", form: "derived" },
      { harness: "copilot", text: "~$0.1005", form: "reported" },
    ]);
    expect(c.title).toMatch(/via `claude` \(derived\)/);
    expect(c.title).toMatch(/via `copilot` \(reported\)/);
    expect(c.title).toContain(COST_REPORTED_NOTE);
    // The reason leads the tooltip — the absence is the headline, not a footnote.
    expect(c.title.indexOf("Cost unavailable")).toBeLessThan(c.title.indexOf("via `claude`"));
  });

  it("frames a reported slice as reported even under an unavailable total (#615 AC)", () => {
    // copilot + opencode: no total, and the only slice is a reported one. The
    // Claude-Code estimate wording must not appear — there is no derived cost here.
    const c = formatEstCost(0, false, [], ["opencode"], [
      { harness: "copilot", usd: 1.0, form: "reported", partial: false, unpriced_models: [] },
    ]);
    expect(c.text).toBe("—");
    expect(c.title).not.toContain(COST_ESTIMATE_NOTE);
    expect(c.title).toContain(COST_REPORTED_NOTE);
  });

  it("keeps a bare '—' when an unavailable Run has no computable slice (#553)", () => {
    // An all-opencode Run: nothing to ventilate, so nothing is invented.
    const c = formatEstCost(0, false, [], ["opencode"], []);
    expect(c.text).toBe("—");
    expect(c.ventilation).toBeUndefined();
    expect(c.title).toMatch(/cost unavailable/i);
  });

  it("shows a pi slice reported in dollars without `~`, and the mixed total with it (#707)", () => {
    // claude (derived estimate) + pi (reported, constant 1.0): the pi slice is an
    // exact figure and drops the `~`; the total still contains an estimate and keeps it.
    const c = formatEstCost(5.02, false, [], [], [
      { harness: "claude", usd: 5.0, form: "derived", partial: false, unpriced_models: [] },
      { harness: "pi", usd: 0.020682, form: "reported", partial: false, unpriced_models: [], reported_in_usd: true },
    ]);
    expect(c.text).toBe("~$5.02");
    expect(c.ventilation).toEqual([
      { harness: "claude", text: "~$5.00", form: "derived" },
      { harness: "pi", text: "$0.0207", form: "reported" },
    ]);
    expect(c.title).toMatch(/\$0\.0207 via `pi` \(reported\)/);
    expect(c.title).toContain(COST_REPORTED_IN_USD_NOTE);
    expect(c.title).not.toContain(COST_REPORTED_NOTE);
    expect(c.title).toContain(COST_ESTIMATE_NOTE);
  });

  it("drops the `~` on the total of an all-pi Run, and keeps it for copilot (#707)", () => {
    const pi = formatEstCost(0.020682, false, [], [], [
      { harness: "pi", usd: 0.020682, form: "reported", partial: false, unpriced_models: [], reported_in_usd: true },
    ]);
    expect(pi.text).toBe("$0.0207");
    expect(pi.dagger).toBe(false);
    expect(pi.title).not.toContain(COST_ESTIMATE_NOTE);
    // A copilot slice is reported but CONVERTED (nano-AIU × a constant): still `~`.
    const copilot = formatEstCost(1.0, false, [], [], [
      { harness: "copilot", usd: 1.0, form: "reported", partial: false, unpriced_models: [] },
    ]);
    expect(copilot.text).toBe("~$1.00");
    // And a pi slice keeps its exact figure under an unavailable total (ADR-0052 §3).
    const mixed = formatEstCost(0, false, [], ["opencode"], [
      { harness: "pi", usd: 0.5, form: "reported", partial: false, unpriced_models: [], reported_in_usd: true },
    ]);
    expect(mixed.text).toBe("—");
    expect(mixed.ventilation).toEqual([{ harness: "pi", text: "$0.5000", form: "reported" }]);
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
