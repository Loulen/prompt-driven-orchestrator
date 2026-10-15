import { describe, it, expect } from "vitest";
import {
  displayedDurations,
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
    ["turn_ended", true],
    ["children_pending", false],
    ["child_awaiting", false],
    [null, false],
  ] as const)("cause %s is a declared wait: %s", (cause, expected) => {
    expect(isDeclaredWaitCause(cause)).toBe(expected);
  });
});

describe("displayedDurations (#980 FP)", () => {
  it("keeps Total = Active + Waiting in whole seconds once each is rounded", () => {
    // 198.4 s = 62.9 s + 135.5 s: rounding each on its own would print 198 != 63 + 136.
    const d = displayedDurations({ total: 198_400, active: 62_900, waiting: 135_500 }, false);
    expect(d).toEqual({ total: 198_000, active: 62_000, waiting: 136_000 });
    expect(d.active + d.waiting).toBe(d.total);
  });

  it("rounds Total and Waiting as Stats -> Performance does", () => {
    // The FP's finished iteration: Stats reads 4m13s, Waiting 2m16s, Active 1m57s.
    const d = displayedDurations({ total: 252_738, active: 117_046, waiting: 135_692 }, false);
    expect(d).toEqual({ total: 253_000, active: 117_000, waiting: 136_000 });
  });

  it("keeps Active still while a wait is open, whatever the tick", () => {
    const active = 40_300;
    for (let x = 0; x < 2_000; x += 100) {
      const total = 68_800 + x;
      const d = displayedDurations({ total, active, waiting: total - active }, true);
      expect(d.active).toBe(40_000);
      expect(d.active + d.waiting).toBe(d.total);
    }
  });

  it("keeps Waiting still while the iteration works", () => {
    const waiting = 135_500;
    for (let x = 0; x < 2_000; x += 100) {
      const total = 198_400 + x;
      const d = displayedDurations({ total, active: total - waiting, waiting }, false);
      expect(d.waiting).toBe(136_000);
    }
  });
});
