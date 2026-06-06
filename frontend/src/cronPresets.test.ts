import { describe, expect, it } from "vitest";
import { presetToCron, cronToPreset, humanizeCron, parseDailyTime, CRON_PRESETS } from "./cronPresets";

describe("cronPresets", () => {
  it("compiles the every-15-min preset to a cron expression", () => {
    expect(presetToCron("every_15_min")).toBe("*/15 * * * *");
  });

  it("compiles the hourly preset", () => {
    expect(presetToCron("hourly")).toBe("0 * * * *");
  });

  it("compiles the daily preset with a chosen time", () => {
    expect(presetToCron("daily", { hour: 9, minute: 0 })).toBe("0 9 * * *");
    expect(presetToCron("daily", { hour: 18, minute: 30 })).toBe("30 18 * * *");
  });

  it("round-trips a known preset cron back to its preset id", () => {
    expect(cronToPreset("*/15 * * * *")).toBe("every_15_min");
    expect(cronToPreset("0 * * * *")).toBe("hourly");
    expect(cronToPreset("0 9 * * *")).toBe("daily");
  });

  it("returns custom for an arbitrary cron expression", () => {
    expect(cronToPreset("17 3 * * 1")).toBe("custom");
  });

  it("humanizes common schedules", () => {
    expect(humanizeCron("*/15 * * * *")).toBe("every 15 min");
    expect(humanizeCron("0 * * * *")).toBe("hourly");
    expect(humanizeCron("0 9 * * *")).toBe("daily at 09:00");
  });

  it("falls back to the raw expression for unknown schedules", () => {
    expect(humanizeCron("17 3 * * 1")).toBe("17 3 * * 1");
  });

  it("parses a daily cron's time-of-day, tolerating irregular whitespace", () => {
    expect(parseDailyTime("30 18 * * *")).toEqual({ hour: 18, minute: 30 });
    // Same normalization as cronToPreset: extra spaces still parse.
    expect(parseDailyTime("0  9 * * *")).toEqual({ hour: 9, minute: 0 });
    expect(parseDailyTime("*/15 * * * *")).toBeNull();
    expect(parseDailyTime("17 3 * * 1")).toBeNull();
  });

  it("exposes the selectable presets", () => {
    const ids = CRON_PRESETS.map((p) => p.id);
    expect(ids).toContain("every_15_min");
    expect(ids).toContain("hourly");
    expect(ids).toContain("daily");
  });
});
