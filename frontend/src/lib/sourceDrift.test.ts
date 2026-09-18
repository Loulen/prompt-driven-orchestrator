import { describe, it, expect } from "vitest";
import type { SourceDrift } from "../types";
import { driftAxes, driftChip, driftTooltip } from "./sourceDrift";

/**
 * #803/ADR-0070 §4 — how the dérive de la source reads on a Run. Pure functions
 * over the daemon's answer plus the outcome of the last EXPLICIT fetch; nothing
 * here knows how to reach the network, which is the point.
 */

const NOW = Date.parse("2026-09-18T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const HOUR = 3_600_000;

const available = (over: Partial<Extract<SourceDrift, { state: "available" }>> = {}) =>
  ({
    state: "available",
    source_branch: "main",
    fork: "a1b2c3d",
    ahead: 1,
    behind: 3,
    local_behind: 1,
    upstream: "origin/main",
    upstream_behind: 3,
    last_fetch_at: ago(2 * HOUR),
    ...over,
  }) satisfies SourceDrift;

const failed = { kind: "network", message: "could not resolve host" };

describe("driftChip", () => {
  it("shows both axes when the numbers are good", () => {
    expect(driftChip(available(), null)).toEqual({ kind: "drift", ahead: 1, behind: 3 });
  });

  it("shows nothing at all before the drift has been read", () => {
    expect(driftChip(null, null)).toBeNull();
  });

  /**
   * The `m` axis is dated from a fetch that never landed, so it becomes `?` — but
   * `n` survives: the Run's own commits live on a local branch no remote has an
   * opinion about. A blanket "unavailable" would throw away a number we know.
   */
  it("keeps the Run's own count when the last fetch failed", () => {
    expect(driftChip(available(), failed)).toEqual({ kind: "unknown", ahead: 1 });
    expect(driftAxes({ kind: "unknown", ahead: 1 })).toEqual({ ahead: "1↑", behind: "?" });
  });

  /**
   * An archived Run whose branch was cleaned up: the expected end of a Run's life,
   * named rather than reported as an error — and never a fabricated zero.
   */
  it("is unavailable, with a reason, once the Run's branch is gone", () => {
    const chip = driftChip({ state: "unavailable", reason: "branch deleted" }, null);
    expect(chip).toEqual({ kind: "unavailable", reason: "branch deleted" });
    expect(driftAxes(chip!)).toBeNull();
  });
});

describe("driftTooltip", () => {
  it("says what each number counts, splits the source side, and names the consequence", () => {
    const lines = driftTooltip(available(), "20260918-110122-eaea593", null, NOW);
    expect(lines[0]).toContain("1↑ commit on");
    // ONE number on the chip, the split in the tooltip (spec #801 Q3): this is
    // the line that keeps the max from looking like an arbitrary number.
    expect(lines[1]).toBe("3↓ arrived on the source: main +1 · origin/main +3");
    expect(lines[2]).toBe("Fork a1b2c3d · local refs, remote as of fetch 2 h ago");
    // The consequence, not the measurement — the reason anyone reads the chip.
    expect(lines[3]).toBe("Merging back will need a merge or rebase.");
  });

  it("promises no merge when nothing arrived on the source", () => {
    const lines = driftTooltip(available({ behind: 0, local_behind: 0, upstream_behind: 0 }), "x", null, NOW);
    expect(lines.some((l) => l.includes("merge or rebase"))).toBe(false);
  });

  it("does not split a source that is itself a tracking ref", () => {
    const lines = driftTooltip(
      available({
        source_branch: "origin/main",
        local_behind: null,
        upstream: null,
        upstream_behind: null,
        behind: 2,
      }),
      "x",
      null,
      NOW,
    );
    expect(lines[1]).toBe("2↓ arrived on the source: origin/main +2");
  });

  /** Honest about WHICH half is stale, and about what it still knows. */
  it("names the failed fetch and keeps the local side", () => {
    const lines = driftTooltip(available(), "x", failed, NOW);
    expect(lines[1]).toContain("could not resolve host");
    expect(lines[2]).toBe("Local main has 1 since the fork.");
  });

  it("admits when no fetch is on record", () => {
    const lines = driftTooltip(available({ last_fetch_at: null }), "x", null, NOW);
    expect(lines.some((l) => l.includes("no fetch on record"))).toBe(true);
  });

  it("carries the reason for an unavailable drift", () => {
    expect(driftTooltip({ state: "unavailable", reason: "branch deleted" }, "x", null, NOW)).toEqual(
      ["branch deleted"],
    );
  });
});
