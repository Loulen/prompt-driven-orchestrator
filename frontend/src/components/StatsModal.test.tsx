import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchStatsOverviewMock = vi.fn();
const fetchStatsCostMock = vi.fn();
const fetchStatsPerformanceMock = vi.fn();
const syncCostPricesMock = vi.fn();

// Every api function StatsModal (or anything it renders) touches MUST be in this
// factory: Vitest 4 wraps the return in a Proxy whose `get` trap throws, and the SSR
// transform rewrites calls into member accesses — so a missing key does not break at
// import, it throws at FIRST ACCESS with `No "<name>" export is defined`.
vi.mock("../api", () => ({
  fetchStatsOverview: (...args: unknown[]) => fetchStatsOverviewMock(...args),
  fetchStatsCost: (...args: unknown[]) => fetchStatsCostMock(...args),
  fetchStatsPerformance: (...args: unknown[]) => fetchStatsPerformanceMock(...args),
  syncCostPrices: (...args: unknown[]) => syncCostPricesMock(...args),
}));

// recharts is heavy and code-split behind `React.lazy`; the charts are strictly
// presentational and irrelevant to what this file asserts (the sync button lives in
// StatsModal precisely because StatsCharts has no access to the refetch).
vi.mock("./StatsCharts", () => ({
  default: () => <div data-testid="stats-charts-stub" />,
}));

import StatsModal from "./StatsModal";
import type { StatsCost, StatsOverview, SyncCostPricesReport } from "../types";

const OVERVIEW: StatsOverview = {
  buckets: ["2026-07-30"],
  runs: [{ bucket: "2026-07-30", count: 1 }],
  errors: [],
  sessions: [],
  session_harnesses: [],
  sessions_by_period: [],
  sessions_by_pipeline: [],
  fires_by_pipeline: [],
  triggers_created_runs: { fired: 0, distinct_triggers: 0, enabled_triggers: 0 },
};

const COST: StatsCost = {
  harnesses: [],
  total: {
    usd: null,
    average_usd: null,
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
  by_project: [],
  resolved: [],
};

function report(overrides: Partial<SyncCostPricesReport> = {}): SyncCostPricesReport {
  return {
    ok: true,
    source: "https://models.dev/api.json",
    fetched_at: "2026-07-30T14:12:03Z",
    rows: 15,
    added: ["claude-fable-5", "claude-opus-5"],
    updated: ["claude-sonnet-5"],
    unchanged: 12,
    rejected: [],
    shadowed_by_manual: [],
    ...overrides,
  };
}

/** Open the modal and switch to the Cost tab, where the sync button lives. */
async function openCostTab() {
  const user = userEvent.setup();
  render(<StatsModal open onClose={() => {}} />);
  await user.click(await screen.findByTestId("stats-tab-cost"));
  await user.click(screen.getByTestId("stats-pricing-trigger"));
  return user;
}

beforeEach(() => {
  fetchStatsOverviewMock.mockReset().mockResolvedValue(OVERVIEW);
  fetchStatsCostMock.mockReset().mockResolvedValue(COST);
  fetchStatsPerformanceMock.mockReset().mockResolvedValue({
    harnesses: [],
    total: { harnesses: [] },
    infrastructure_total: { harnesses: [] },
    by_pipeline: [],
    infrastructure: [],
  });
  syncCostPricesMock.mockReset().mockResolvedValue(report());
});

describe("StatsModal — price sync (#427, ADR-0034)", () => {
  it("keeps Sync costs inside the collapsed Pricing details panel", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    expect(await screen.findByTestId("stats-tab-runs")).toBeInTheDocument();
    expect(screen.queryByTestId("stats-sync-prices")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("stats-tab-cost"));
    expect(screen.getByTestId("stats-pricing-trigger")).toBeInTheDocument();
    expect(screen.queryByTestId("stats-sync-prices")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("stats-pricing-trigger"));
    expect(screen.getByTestId("stats-sync-prices")).toBeInTheDocument();
  });

  it("renders a readable report on success, naming what was repaired", async () => {
    const user = await openCostTab();
    await user.click(screen.getByTestId("stats-sync-prices"));

    const box = await screen.findByTestId("stats-sync-report");
    expect(box).toHaveTextContent("claude-fable-5");
    expect(box).toHaveTextContent("claude-opus-5");
    expect(box).toHaveTextContent("claude-sonnet-5");
    expect(box).toHaveTextContent("15 price row(s)");
    expect(screen.queryByTestId("stats-sync-noop")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stats-sync-error")).not.toBeInTheDocument();
  });

  describe("StatsModal — full-screen Stats window (#638)", () => {
    it("covers the application and exposes Performance as the fifth side-rail section", async () => {
      render(<StatsModal open onClose={() => {}} />);

      expect(await screen.findByTestId("stats-modal")).toHaveClass("h-screen", "w-screen");
      const rail = screen.getByRole("tablist", { name: "Stats sections" });
      expect(rail).toHaveClass("flex-col");
      expect(screen.getAllByRole("tab")).toHaveLength(5);
      expect(screen.getByTestId("stats-tab-performance")).toHaveTextContent("Performance");
      expect(screen.getByRole("group", { name: "Period" })).toBeInTheDocument();
    });

    it("loads Performance only when opened and refreshes it explicitly", async () => {
      const user = userEvent.setup();
      render(<StatsModal open onClose={() => {}} />);
      await waitFor(() => expect(fetchStatsOverviewMock).toHaveBeenCalledTimes(1));
      expect(fetchStatsPerformanceMock).not.toHaveBeenCalled();

      await user.click(screen.getByTestId("stats-tab-performance"));
      await waitFor(() => expect(fetchStatsPerformanceMock).toHaveBeenCalledTimes(1));
      await user.click(screen.getByTestId("stats-refresh"));
      await waitFor(() => expect(fetchStatsPerformanceMock).toHaveBeenCalledTimes(2));
    });

    it("refreshes visible data without blanking it and advances the computed time", async () => {
      const user = userEvent.setup();
      render(<StatsModal open onClose={() => {}} />);
      await waitFor(() => expect(fetchStatsOverviewMock).toHaveBeenCalledTimes(1));
      expect(screen.getByTestId("stats-charts-stub")).toBeInTheDocument();
      const before = screen.getByTestId("stats-computed-at").textContent;

      await user.click(screen.getByTestId("stats-refresh"));
      await waitFor(() => expect(fetchStatsOverviewMock).toHaveBeenCalledTimes(2));
      expect(screen.getByTestId("stats-charts-stub")).toBeInTheDocument();
      expect(screen.getByTestId("stats-computed-at")).toHaveTextContent(/Computed/);
      expect(before).not.toBeNull();
    });

    it("closes Pricing details before closing Stats on Escape", async () => {
      const user = userEvent.setup();
      const onClose = vi.fn();
      render(<StatsModal open onClose={onClose} />);
      await user.click(screen.getByTestId("stats-tab-cost"));
      await user.click(screen.getByTestId("stats-pricing-trigger"));
      expect(screen.getByTestId("stats-pricing-details")).toBeInTheDocument();

      await user.keyboard("{Escape}");
      expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
      expect(onClose).not.toHaveBeenCalled();

      await user.keyboard("{Escape}");
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it("lets an open coverage tooltip consume Escape before Stats", async () => {
      const user = userEvent.setup();
      const onClose = vi.fn();
      const tooltip = document.createElement("div");
      tooltip.dataset.testid = "tooltip-content";
      tooltip.dataset.state = "delayed-open";
      document.body.appendChild(tooltip);
      render(<StatsModal open onClose={onClose} />);

      await user.keyboard("{Escape}");
      expect(onClose).not.toHaveBeenCalled();
      tooltip.remove();
    });

    it("shows the resolved price table only inside Pricing details", async () => {
      fetchStatsCostMock.mockResolvedValue({
        ...COST,
        resolved: [{ key: "claude-opus-5", tier: "fetched", input: 5, output: 25 }],
      });
      const user = userEvent.setup();
      render(<StatsModal open onClose={() => {}} />);
      await user.click(screen.getByTestId("stats-tab-cost"));
      expect(screen.queryByText("claude-opus-5")).not.toBeInTheDocument();

      await user.click(screen.getByTestId("stats-pricing-trigger"));
      expect(await screen.findByText("claude-opus-5")).toBeInTheDocument();
      expect(screen.getByText("$5/$25 /MTok")).toBeInTheDocument();
    });
  });

  it("says when the manual tier shadows a fetched price", async () => {
    // A sync must never silently erase a hand correction — it is REPORTED.
    syncCostPricesMock.mockResolvedValue(
      report({ shadowed_by_manual: ["claude-opus-4-8"] }),
    );
    const user = await openCostTab();
    await user.click(screen.getByTestId("stats-sync-prices"));

    const box = await screen.findByTestId("stats-sync-report");
    expect(box).toHaveTextContent("claude-opus-4-8");
    expect(box).toHaveTextContent(/models\.yaml/);
  });

  it("renders a noop as its reason, not as a success box", async () => {
    // ADR-0025: never a blind `{ok:true}`.
    syncCostPricesMock.mockResolvedValue(
      report({
        noop: true,
        reason: "table already up to date — 15 row(s) from the source, none changed",
      }),
    );
    const user = await openCostTab();
    await user.click(screen.getByTestId("stats-sync-prices"));

    expect(await screen.findByTestId("stats-sync-noop")).toHaveTextContent(
      /already up to date/,
    );
    expect(screen.queryByTestId("stats-sync-report")).not.toBeInTheDocument();
  });

  it("surfaces a failure naming the source, and keeps the tab usable", async () => {
    // The daemon answers 502 with the URL (ADR-0030: an explicitly requested effect
    // that fails is a hard error that NAMES the source, never a silent fallback).
    syncCostPricesMock.mockRejectedValue(
      new Error("price source unreachable: https://models.dev/api.json: request failed"),
    );
    const user = await openCostTab();
    await user.click(screen.getByTestId("stats-sync-prices"));

    const err = await screen.findByTestId("stats-sync-error");
    expect(err).toHaveTextContent("https://models.dev/api.json");
    expect(screen.queryByTestId("stats-sync-report")).not.toBeInTheDocument();
    // The button is usable again — a failure is not a dead end.
    expect(screen.getByTestId("stats-sync-prices")).not.toBeDisabled();
  });

  it("disables the button while a sync is in flight", async () => {
    // Client-side guard on top of the daemon's 409 (precedent: `guardTesting`).
    let release: (r: SyncCostPricesReport) => void = () => {};
    syncCostPricesMock.mockReturnValue(
      new Promise<SyncCostPricesReport>((resolve) => {
        release = resolve;
      }),
    );
    const user = await openCostTab();
    const button = screen.getByTestId("stats-sync-prices");
    await user.click(button);

    expect(button).toBeDisabled();
    expect(button).toHaveTextContent(/syncing/i);

    release(report());
    await waitFor(() => expect(button).not.toBeDisabled());
  });

  it("refetches /stats/cost after a successful sync (the bumped reloadKey)", async () => {
    // Without this the button would repair the table and lie about it: there is no
    // polling of `/stats/cost`, so nothing else would move the number.
    const user = await openCostTab();
    await waitFor(() => expect(fetchStatsCostMock).toHaveBeenCalledTimes(1));

    await user.click(screen.getByTestId("stats-sync-prices"));
    await waitFor(() => expect(fetchStatsCostMock).toHaveBeenCalledTimes(2));

    // And the reload key never reaches the API: the call keeps its 3-arg shape, which
    // `useStats.test.ts` asserts exactly (Vitest compares arity strictly). The modal
    // owns the period, so assert the SHAPE, not literal dates.
    const args = fetchStatsCostMock.mock.calls.at(-1)!;
    expect(args).toHaveLength(3);
    expect(args[2]).toBe("day"); // the 30d default preset's bucket
  });
});
