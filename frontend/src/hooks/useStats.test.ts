import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useStats } from "./useStats";
import * as api from "../api";
import type { StatsOverview, StatsCost } from "../types";

vi.mock("../api", () => ({
  fetchStatsOverview: vi.fn(),
  fetchStatsCost: vi.fn(),
}));

const OVERVIEW: StatsOverview = {
  buckets: ["2026-07-15"],
  runs: [{ bucket: "2026-07-15", count: 2 }],
  errors: [],
  sessions: [],
  fires_by_pipeline: [],
  triggers_created_runs: { fired: 0, distinct_triggers: 0, enabled_triggers: 1 },
};

const COST: StatsCost = { by_period: [], by_pipeline: [], by_project: [] };

beforeEach(() => {
  vi.mocked(api.fetchStatsOverview).mockReset().mockResolvedValue(OVERVIEW);
  vi.mocked(api.fetchStatsCost).mockReset().mockResolvedValue(COST);
});

describe("useStats (#377)", () => {
  it("fetches overview eagerly on open, but not cost", async () => {
    const { result } = renderHook(() => useStats(true, "F", "T", "day", false));
    await waitFor(() => expect(result.current.overview).toEqual(OVERVIEW));
    expect(api.fetchStatsOverview).toHaveBeenCalledWith("F", "T", "day");
    expect(api.fetchStatsCost).not.toHaveBeenCalled();
  });

  it("does not fetch anything while closed", async () => {
    renderHook(() => useStats(false, "F", "T", "day", true));
    await Promise.resolve();
    expect(api.fetchStatsOverview).not.toHaveBeenCalled();
    expect(api.fetchStatsCost).not.toHaveBeenCalled();
  });

  it("fetches cost lazily, only once the cost tab is active (two-endpoint split)", async () => {
    const { result, rerender } = renderHook(
      ({ costActive }) => useStats(true, "F", "T", "day", costActive),
      { initialProps: { costActive: false } },
    );
    await waitFor(() => expect(result.current.overview).toEqual(OVERVIEW));
    expect(api.fetchStatsCost).not.toHaveBeenCalled();

    rerender({ costActive: true });
    await waitFor(() => expect(result.current.cost).toEqual(COST));
    expect(api.fetchStatsCost).toHaveBeenCalledWith("F", "T", "day");
  });

  it("refetches overview when the period changes", async () => {
    const { rerender } = renderHook(
      ({ bucket }) => useStats(true, "F", "T", bucket, false),
      { initialProps: { bucket: "day" } },
    );
    await waitFor(() => expect(api.fetchStatsOverview).toHaveBeenCalledTimes(1));
    rerender({ bucket: "week" });
    await waitFor(() => expect(api.fetchStatsOverview).toHaveBeenCalledTimes(2));
    expect(api.fetchStatsOverview).toHaveBeenLastCalledWith("F", "T", "week");
  });

  it("surfaces an overview fetch error", async () => {
    vi.mocked(api.fetchStatsOverview).mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => useStats(true, "F", "T", "day", false));
    await waitFor(() => expect(result.current.error).toBe("boom"));
  });

  it("surfaces a cost fetch error only for the cost class", async () => {
    vi.mocked(api.fetchStatsCost).mockRejectedValueOnce(new Error("cost-boom"));
    const { result } = renderHook(() => useStats(true, "F", "T", "day", true));
    await waitFor(() => expect(result.current.costError).toBe("cost-boom"));
    expect(result.current.error).toBeNull();
  });
});
