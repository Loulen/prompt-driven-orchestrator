// #723 — unit tests for the shared orchestration read model: the counters the
// pastilles render and the cost vocabulary they speak.
import { describe, expect, it } from "vitest";
import {
  childCostKnown,
  childCostText,
  countChildren,
  totalChildren,
  type RunChildEntry,
} from "./orchestration";

function child(
  status: RunChildEntry["status"],
  cost?: RunChildEntry["cost"],
  stalled?: boolean,
): RunChildEntry {
  return { run_id: `run-${status}-${Math.random()}`, pipeline_name: "p", status, cost, stalled };
}

describe("countChildren (#723)", () => {
  it("buckets failed / running / finished with the pills totalling the children", () => {
    const counts = countChildren([
      child("completed"),
      child("skipped"),
      child("failed"),
      child("running"),
      child("awaiting_user"),
      child("paused"),
    ]);
    expect(counts).toEqual({ finished: 2, failed: 1, stale: 0, running: 3 });
    expect(totalChildren(counts)).toBe(6);
  });

  it("#783 — a live stalled child is STALE, carved out of running (disjoint sets)", () => {
    const counts = countChildren([
      child("running", undefined, true),
      child("running", undefined, false),
      child("awaiting_user", undefined, true),
      // A terminal run can never be stale, whatever the flag says.
      child("completed", undefined, true),
      child("failed", undefined, true),
    ]);
    expect(counts).toEqual({ finished: 1, failed: 1, stale: 2, running: 1 });
    expect(totalChildren(counts)).toBe(5);
  });

  it("counts awaiting_user and paused as RUNNING (blue) — design decision 3", () => {
    // The blue pill is "still to settle"; a failed child is red. The two sets
    // stay disjoint, so awaiting/paused children never read finished.
    const counts = countChildren([child("awaiting_user"), child("paused")]);
    expect(counts.running).toBe(2);
    expect(counts.finished).toBe(0);
    expect(counts.failed).toBe(0);
  });

  it("an empty list yields all-zero counts — the pills hide everywhere", () => {
    expect(totalChildren(countChildren([]))).toBe(0);
  });
});

describe("childCostText / childCostKnown (#723)", () => {
  it("renders — for an absent cost, never $0", () => {
    expect(childCostText(undefined)).toBe("—");
  });

  it("renders — for a child with no costable session (usd 0, no slice)", () => {
    expect(childCostText({ usd: 0, partial: false })).toBe("—");
    expect(childCostKnown({ usd: 0, partial: false })).toBeNull();
  });

  it("renders — when a harness has no cost source, the sum is refused", () => {
    const cost = { usd: 1.5, partial: false, uncosted_harnesses: ["opencode"] };
    expect(childCostText(cost)).toBe("—");
    expect(childCostKnown(cost)).toBeNull();
  });

  it("renders a derived estimate with the ~ frame, and its dollars as summable", () => {
    const cost = { usd: 0.5, partial: false };
    expect(childCostText(cost)).toBe("~$0.5000");
    expect(childCostKnown(cost)).toBe(0.5);
  });

  it("renders an exact reported total without the ~", () => {
    const cost = {
      usd: 2,
      partial: false,
      by_harness: [{ harness: "pi", usd: 2, form: "reported" as const, partial: false, unpriced_models: [], reported_in_usd: true }],
    };
    expect(childCostText(cost)).toBe("$2.00");
    expect(childCostKnown(cost)).toBe(2);
  });
});
