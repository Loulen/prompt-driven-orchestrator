import { describe, it, expect } from "vitest";
import { iterationIsLive, iterationTotalMs } from "./iterationDuration";

const START = "2026-01-01T00:00:00Z";
const NOW = Date.parse("2026-01-01T00:05:00Z");

describe("iterationTotalMs (#979)", () => {
  it("measures a finished iteration from start to end, ignoring now", () => {
    expect(
      iterationTotalMs(
        { status: "completed", started_at: START, completed_at: "2026-01-01T00:12:04Z" },
        NOW,
      ),
    ).toBe(724_000);
  });

  it.each(["running", "awaiting_user"] as const)("runs up to now while %s", (status) => {
    expect(iterationTotalMs({ status, started_at: START, completed_at: null }, NOW)).toBe(
      300_000,
    );
  });

  it("has no duration without a start", () => {
    expect(iterationTotalMs({ status: "skipped", started_at: null, completed_at: null }, NOW)).toBeNull();
  });

  it("has no duration for a stopped iteration without a readable end", () => {
    expect(
      iterationTotalMs({ status: "interrupted", started_at: START, completed_at: null }, NOW),
    ).toBeNull();
    expect(
      iterationTotalMs({ status: "completed", started_at: START, completed_at: "garbage" }, NOW),
    ).toBeNull();
  });

  it("never goes negative on clock skew", () => {
    expect(
      iterationTotalMs({ status: "running", started_at: START, completed_at: null }, NOW - 600_000),
    ).toBe(0);
  });

  it("only running and awaiting iterations are live", () => {
    expect(iterationIsLive("running")).toBe(true);
    expect(iterationIsLive("awaiting_user")).toBe(true);
    expect(iterationIsLive("completed")).toBe(false);
    expect(iterationIsLive("pending")).toBe(false);
  });
});
