import { describe, expect, it } from "vitest";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import {
  ANY_TIME,
  rangeDays,
  runMatchesDate,
  startDateLabel,
  type RunDateFilter,
} from "./runDateFilter";

/* The Date axis: « Active » keeps a Run with activity in the range, « Started »
   one started in it. Days are the browser's: the instants below are built in
   local time so the test holds in any time zone. */

const local = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).toISOString();
// Wednesday 2026-10-14, mid-afternoon.
const NOW = new Date(2026, 9, 14, 15);

const run = (over: Partial<RunListEntry>): RunListEntry => ({
  run_id: "r",
  pipeline_name: "p",
  status: "running",
  stalled: false,
  effective_repo: "/repo",
  started_at: null,
  activity: [],
  ...over,
});

// Started on Monday the 12th, resumed on Wednesday the 14th; nothing on Tuesday.
const mondayWednesday = run({
  started_at: local(2026, 10, 12, 9),
  activity: [
    { from: local(2026, 10, 12, 9), to: local(2026, 10, 12, 10) },
    { from: local(2026, 10, 14, 8), to: local(2026, 10, 14, 9) },
  ],
});

const day = (mode: RunDateFilter["mode"], d: string): RunDateFilter => ({
  mode,
  range: { from: d, to: d },
});

describe("runMatchesDate", () => {
  it("any time keeps every Run", () => {
    expect(runMatchesDate(run({}), ANY_TIME, NOW)).toBe(true);
  });

  it("Active keeps a Run on each day it was active, not on the day between", () => {
    expect(runMatchesDate(mondayWednesday, day("active", "2026-10-12"), NOW)).toBe(true);
    expect(runMatchesDate(mondayWednesday, day("active", "2026-10-13"), NOW)).toBe(false);
    expect(runMatchesDate(mondayWednesday, day("active", "2026-10-14"), NOW)).toBe(true);
  });

  it("Started keeps a Run only on the day it started", () => {
    expect(runMatchesDate(mondayWednesday, day("started", "2026-10-12"), NOW)).toBe(true);
    expect(runMatchesDate(mondayWednesday, day("started", "2026-10-14"), NOW)).toBe(false);
  });

  it("a span over midnight is active on both days", () => {
    const night = run({
      started_at: local(2026, 10, 12, 23),
      activity: [{ from: local(2026, 10, 12, 23), to: local(2026, 10, 13, 0) }],
    });
    expect(runMatchesDate(night, day("active", "2026-10-12"), NOW)).toBe(true);
    expect(runMatchesDate(night, day("active", "2026-10-13"), NOW)).toBe(true);
  });

  it("the presets roll from today", () => {
    const yesterday: RunDateFilter = { mode: "active", range: { preset: "yesterday" } };
    const today: RunDateFilter = { mode: "active", range: { preset: "today" } };
    const week: RunDateFilter = { mode: "started", range: { preset: "7d" } };
    expect(runMatchesDate(mondayWednesday, today, NOW)).toBe(true);
    expect(runMatchesDate(mondayWednesday, yesterday, NOW)).toBe(false);
    expect(runMatchesDate(mondayWednesday, week, NOW)).toBe(true);
    expect(runMatchesDate(mondayWednesday, { ...week, range: { preset: "today" } }, NOW)).toBe(false);
    expect(rangeDays({ preset: "7d" }, NOW)).toEqual({ from: "2026-10-07", to: "2026-10-14" });
    expect(rangeDays({ preset: "30d" }, NOW)).toEqual({ from: "2026-09-14", to: "2026-10-14" });
  });

  it("a custom range covers both its days, in whichever order they come", () => {
    const range = { from: "2026-10-13", to: "2026-10-12" };
    expect(runMatchesDate(mondayWednesday, { mode: "active", range }, NOW)).toBe(true);
    expect(
      runMatchesDate(mondayWednesday, { mode: "active", range: { from: "2026-10-13", to: "2026-10-13" } }, NOW),
    ).toBe(false);
  });

  it("a Run without activity falls back on its start", () => {
    const quiet = run({ started_at: local(2026, 10, 13) });
    expect(runMatchesDate(quiet, day("active", "2026-10-13"), NOW)).toBe(true);
    expect(runMatchesDate(quiet, day("active", "2026-10-14"), NOW)).toBe(false);
  });
});

describe("labels", () => {
  it("a row shows its start date unless it started today", () => {
    expect(startDateLabel(local(2026, 10, 14, 1), NOW)).toBeNull();
    expect(startDateLabel(local(2026, 10, 12), NOW)).toBe("Oct 12");
    expect(startDateLabel(local(2025, 10, 12), NOW)).toBe("Oct 12, 2025");
    expect(startDateLabel(null, NOW)).toBeNull();
  });
});
