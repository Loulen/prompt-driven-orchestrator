import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useStats } from "./useStats";
import * as api from "../api";
import type { StatsOverview, StatsCost } from "../types";

vi.mock("../api", () => ({
  fetchStatsOverview: vi.fn(),
  fetchStatsCost: vi.fn(),
  fetchStatsPerformance: vi.fn(),
}));

const OVERVIEW: StatsOverview = {
  buckets: ["2026-07-15"],
  runs: [{ bucket: "2026-07-15", count: 2 }],
  errors: [],
  sessions: [],
  session_harnesses: [],
  sessions_by_period: [],
  sessions_by_pipeline: [],
  fires_by_pipeline: [],
  triggers_created_runs: { fired: 0, distinct_triggers: 0, enabled_triggers: 1 },
};

const COST: StatsCost = {
  harnesses: [],
  total: {
    usd: null,
    average_usd: null,
    median_usd: null,
    estimated: true,
    partial: false,
    executions: 0,
    readable: 0,
    unknown: 0,
    unpriced_models: [],
    missing_reasons: [],
    harnesses: [],
  },
  by_period: [],
  by_pipeline: [],
  by_model: [],
  by_project: [],
  resolved: [],
};
const PERFORMANCE = {
  harnesses: [],
  total: { harnesses: [] },
  infrastructure_total: { harnesses: [] },
  by_pipeline: [],
  by_model: [],
  infrastructure: [],
  waited_executions: 0,
  executions: 0,
};

beforeEach(() => {
  vi.mocked(api.fetchStatsOverview).mockReset().mockResolvedValue(OVERVIEW);
  vi.mocked(api.fetchStatsCost).mockReset().mockResolvedValue(COST);
  vi.mocked(api.fetchStatsPerformance).mockReset().mockResolvedValue(PERFORMANCE);
});

describe("useStats (#377)", () => {
  it("fetches overview eagerly on open, but not cost", async () => {
    const { result } = renderHook(() => useStats(true, "F", "T", "day", false, false));
    await waitFor(() => expect(result.current.overview).toEqual(OVERVIEW));
    expect(api.fetchStatsOverview).toHaveBeenCalledWith("F", "T", "day", false);
    expect(api.fetchStatsCost).not.toHaveBeenCalled();
    expect(api.fetchStatsPerformance).not.toHaveBeenCalled();
  });

  it("does not fetch anything while closed", async () => {
    renderHook(() => useStats(false, "F", "T", "day", true, true));
    await Promise.resolve();
    expect(api.fetchStatsOverview).not.toHaveBeenCalled();
    expect(api.fetchStatsCost).not.toHaveBeenCalled();
    expect(api.fetchStatsPerformance).not.toHaveBeenCalled();
  });

  it("fetches cost lazily, only once the cost tab is active (two-endpoint split)", async () => {
    const { result, rerender } = renderHook(
      ({ costActive }) => useStats(true, "F", "T", "day", costActive, false),
      { initialProps: { costActive: false } },
    );
    await waitFor(() => expect(result.current.overview).toEqual(OVERVIEW));
    expect(api.fetchStatsCost).not.toHaveBeenCalled();

    rerender({ costActive: true });
    await waitFor(() => expect(result.current.cost).toEqual(COST));
    expect(api.fetchStatsCost).toHaveBeenCalledWith("F", "T", "day", false);
  });

  it("fetches performance lazily and does not refetch when returning to the tab", async () => {
    const { result, rerender } = renderHook(
      ({ active }) => useStats(true, "F", "T", "day", false, active),
      { initialProps: { active: false } },
    );
    expect(api.fetchStatsPerformance).not.toHaveBeenCalled();

    rerender({ active: true });
    await waitFor(() =>
      expect(result.current.performance).toEqual(PERFORMANCE),
    );
    expect(api.fetchStatsPerformance).toHaveBeenCalledWith(
      "F",
      "T",
      false,
      false,
    );

    rerender({ active: false });
    rerender({ active: true });
    await Promise.resolve();
    expect(api.fetchStatsPerformance).toHaveBeenCalledTimes(1);
  });

  it("bypasses the performance memo after explicit refresh", async () => {
    const { rerender } = renderHook(
      ({ reloadKey }) => useStats(true, "F", "T", "day", false, true, reloadKey),
      { initialProps: { reloadKey: 0 } },
    );
    await waitFor(() =>
      expect(api.fetchStatsPerformance).toHaveBeenCalledWith(
        "F",
        "T",
        false,
        false,
      ),
    );

    rerender({ reloadKey: 1 });
    await waitFor(() =>
      expect(api.fetchStatsPerformance).toHaveBeenLastCalledWith(
        "F",
        "T",
        true,
        false,
      ),
    );
  });

  it("does not refetch cost when returning from another section", async () => {
    const { rerender } = renderHook(
      ({ costActive }) => useStats(true, "F", "T", "day", costActive, false),
      { initialProps: { costActive: true } },
    );
    await waitFor(() => expect(api.fetchStatsCost).toHaveBeenCalledTimes(1));

    rerender({ costActive: false });
    rerender({ costActive: true });
    await Promise.resolve();

    expect(api.fetchStatsCost).toHaveBeenCalledTimes(1);
  });

  it("refetches overview when the period changes", async () => {
    const { rerender } = renderHook(
      ({ bucket }) => useStats(true, "F", "T", bucket, false, false),
      { initialProps: { bucket: "day" } },
    );
    await waitFor(() => expect(api.fetchStatsOverview).toHaveBeenCalledTimes(1));
    rerender({ bucket: "week" });
    await waitFor(() =>
      expect(api.fetchStatsOverview).toHaveBeenCalledTimes(2),
    );
    expect(api.fetchStatsOverview).toHaveBeenLastCalledWith(
      "F",
      "T",
      "week",
      false,
    );
  });

  it("surfaces an overview fetch error", async () => {
    vi.mocked(api.fetchStatsOverview).mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => useStats(true, "F", "T", "day", false, false));
    await waitFor(() => expect(result.current.error).toBe("boom"));
  });

  it("surfaces a cost fetch error only for the cost class", async () => {
    vi.mocked(api.fetchStatsCost).mockRejectedValueOnce(new Error("cost-boom"));
    const { result } = renderHook(() => useStats(true, "F", "T", "day", true, false));
    await waitFor(() => expect(result.current.costError).toBe("cost-boom"));
    expect(result.current.error).toBeNull();
  });

  it("surfaces a performance source error separately", async () => {
    vi.mocked(api.fetchStatsPerformance).mockRejectedValueOnce(new Error("journal unreadable"));
    const { result } = renderHook(() => useStats(true, "F", "T", "day", false, true));
    await waitFor(() => expect(result.current.performanceError).toBe("journal unreadable"));
    expect(result.current.error).toBeNull();
  });
});

describe("useStats — « Runs terminés seulement », per tab (#819)", () => {
  it("sends each tab its own cohort", async () => {
    const { result } = renderHook(() =>
      useStats(true, "F", "T", "day", true, true, 0, {
        overview: false,
        cost: false,
        performance: true,
      }),
    );

    await waitFor(() =>
      expect(result.current.performance).toEqual(PERFORMANCE),
    );
    // Performance opens narrowed; Overview and Cost open on every Run.
    expect(api.fetchStatsOverview).toHaveBeenCalledWith("F", "T", "day", false);
    expect(api.fetchStatsCost).toHaveBeenCalledWith("F", "T", "day", false);
    expect(api.fetchStatsPerformance).toHaveBeenCalledWith("F", "T", false, true);
  });

  it("refetches the tab whose cohort flipped, and only that one", async () => {
    const { rerender } = renderHook(
      ({ performance }) =>
        useStats(true, "F", "T", "day", true, true, 0, {
          overview: false,
          cost: false,
          performance,
        }),
      { initialProps: { performance: true } },
    );

    await waitFor(() => expect(api.fetchStatsPerformance).toHaveBeenCalledTimes(1));
    const overviewCalls = vi.mocked(api.fetchStatsOverview).mock.calls.length;
    const costCalls = vi.mocked(api.fetchStatsCost).mock.calls.length;

    // The cohort is part of the request, not a display option: flipping it must
    // refetch rather than reinterpret the payload in hand — and it narrows one
    // section, so it refetches one endpoint.
    rerender({ performance: false });
    await waitFor(() =>
      expect(api.fetchStatsPerformance).toHaveBeenLastCalledWith(
        "F",
        "T",
        false,
        false,
      ),
    );
    expect(api.fetchStatsOverview).toHaveBeenCalledTimes(overviewCalls);
    expect(api.fetchStatsCost).toHaveBeenCalledTimes(costCalls);
  });

  it("refetches Overview alone when the shared cohort of its three tabs flips", async () => {
    const { rerender } = renderHook(
      ({ overview }) =>
        useStats(true, "F", "T", "day", true, true, 0, {
          overview,
          cost: false,
          performance: true,
        }),
      { initialProps: { overview: false } },
    );

    await waitFor(() => expect(api.fetchStatsCost).toHaveBeenCalledTimes(1));
    const costCalls = vi.mocked(api.fetchStatsCost).mock.calls.length;
    const performanceCalls = vi.mocked(api.fetchStatsPerformance).mock.calls.length;

    rerender({ overview: true });
    await waitFor(() =>
      expect(api.fetchStatsOverview).toHaveBeenLastCalledWith(
        "F",
        "T",
        "day",
        true,
      ),
    );
    expect(api.fetchStatsCost).toHaveBeenCalledTimes(costCalls);
    expect(api.fetchStatsPerformance).toHaveBeenCalledTimes(performanceCalls);
  });
});
