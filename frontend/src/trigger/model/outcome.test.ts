import { describe, it, expect } from "vitest";
import { formatFireTs, lastOutcomeTooltip, outcomeDot, outcomeTone, overlapLabel } from "./outcome";
import type { Trigger } from "../../shared/generated/Trigger";

describe("trigger outcome model", () => {
  it("maps every outcome to its status dot and tone", () => {
    expect(outcomeDot("fired")).toBe("bg-st-done");
    expect(outcomeDot("error")).toBe("bg-st-failed");
    expect(outcomeDot("skipped-overlap")).toBe("bg-st-blocked");
    expect(outcomeDot("guard-exit-nonzero")).toBe("bg-st-blocked");
    expect(outcomeDot("guard-error")).toBe("bg-st-blocked");
    expect(outcomeDot(undefined)).toBe("bg-st-skipped");
    expect(outcomeTone("fired")).toBe("done");
    expect(outcomeTone("error")).toBe("failed");
    expect(outcomeTone("guard-error")).toBe("blocked");
    expect(outcomeTone(null)).toBeNull();
  });

  it("says when a Trigger never fired", () => {
    expect(lastOutcomeTooltip({ last_outcome: undefined } as Trigger)).toBe("Never fired");
  });

  it("names the last tick's result in words, and the last fire apart: a skip is not a fire (CORE-232)", () => {
    expect(lastOutcomeTooltip({ last_outcome: "fired", last_fired_at: "2026-11-09T10:25:20.241Z" })).toBe("Last tick: Fired\nLast fired Nov 9, 10:25 UTC");
    // The skip happened later than the last fire: the instant is the fire's, said as such.
    expect(lastOutcomeTooltip({ last_outcome: "skipped-overlap", last_fired_at: "2026-11-09T10:25:20.241Z" })).toBe("Last tick: Skipped, overlap\nLast fired Nov 9, 10:25 UTC");
    expect(lastOutcomeTooltip({ last_outcome: "guard-exit-nonzero", last_fired_at: undefined })).toBe(
      "Last tick: Guard skip\nNever fired",
    );
  });

  it("reads the overlap policy", () => {
    expect(overlapLabel({ overlap_policy: "skip" })).toBe("skip (default)");
    expect(overlapLabel({ overlap_policy: "allow" })).toBe("allow (unlimited concurrent fires)");
    expect(overlapLabel({ overlap_policy: "allow", max_concurrent: 2 })).toBe(
      "allow (max 2 concurrent)",
    );
  });

  it("says the next fire in the time zone its cron is read in", () => {
    expect(formatFireTs("2026-11-01T03:00:00.000Z")).toMatch(/03:00 UTC$/);
    expect(formatFireTs("2026-11-01T03:00:00.000Z", "local")).not.toMatch(/UTC$/);
  });

  it("has no instant for an absent timestamp", () => {
    expect(formatFireTs(undefined)).toBeNull();
    expect(formatFireTs("2026-09-27T09:00:00.000Z")).not.toBeNull();
  });
});
