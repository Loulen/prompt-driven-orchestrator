import { describe, it, expect } from "vitest";
import {
  isDeclaredWaitCause,
  iterationDurations,
  iterationIsLive,
  iterationTotalMs,
} from "./iterationDuration";

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

describe("iterationDurations (#980)", () => {
  it("splits a finished iteration into Active and its closed waits", () => {
    expect(
      iterationDurations(
        {
          status: "completed",
          started_at: START,
          completed_at: "2026-01-01T00:12:04Z",
          waited_ms: 213_000,
        },
        NOW,
      ),
    ).toEqual({ total: 724_000, active: 511_000, waiting: 213_000 });
  });

  it("adds the open wait to Waiting, so Active stays put while Total grows", () => {
    const timing = {
      status: "awaiting_user" as const,
      started_at: START,
      completed_at: null,
      waited_ms: 60_000,
      open_wait_since: "2026-01-01T00:04:00Z",
    };
    const at5 = iterationDurations(timing, NOW);
    const at7 = iterationDurations(timing, NOW + 120_000);
    expect(at5).toEqual({ total: 300_000, active: 180_000, waiting: 120_000 });
    expect(at7).toEqual({ total: 420_000, active: 180_000, waiting: 240_000 });
  });

  it("treats an absent waited_ms (older daemon) as no wait", () => {
    expect(
      iterationDurations({ status: "running", started_at: START, completed_at: null }, NOW),
    ).toEqual({ total: 300_000, active: 300_000, waiting: 0 });
  });

  it("caps Waiting at Total so Total = Active + Waiting even on a clock skew", () => {
    const d = iterationDurations(
      { status: "completed", started_at: START, completed_at: "2026-01-01T00:01:00Z", waited_ms: 90_000 },
      NOW,
    );
    expect(d).toEqual({ total: 60_000, active: 0, waiting: 60_000 });
  });

  it("has no durations without a Total", () => {
    expect(
      iterationDurations({ status: "skipped", started_at: null, completed_at: null }, NOW),
    ).toBeNull();
  });

  it.each([
    ["declared", true],
    ["completion_not_released", true],
    ["children_pending", false],
    ["child_awaiting", false],
    [null, false],
  ] as const)("cause %s is a declared wait: %s", (cause, expected) => {
    expect(isDeclaredWaitCause(cause)).toBe(expected);
  });
});
