import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const fetchStatsOverviewMock = vi.fn();
const fetchStatsCostMock = vi.fn();
const fetchStatsPerformanceMock = vi.fn();
const syncCostPricesMock = vi.fn();
const fetchStatsAbsorptionsMock = vi.fn();
// #891: the few tests about the band's chips render the REAL charts; the rest
// keep the stub below.
let realCharts = false;

// Every api function StatsModal (or anything it renders) touches MUST be in this
// factory: Vitest 4 wraps the return in a Proxy whose `get` trap throws, and the SSR
// transform rewrites calls into member accesses — so a missing key does not break at
// import, it throws at FIRST ACCESS with `No "<name>" export is defined`.
vi.mock("../api/stats", () => ({
  fetchStatsOverview: (...args: unknown[]) => fetchStatsOverviewMock(...args),
  fetchStatsCost: (...args: unknown[]) => fetchStatsCostMock(...args),
  fetchStatsPerformance: (...args: unknown[]) => fetchStatsPerformanceMock(...args),
}));
vi.mock("../api/prices", () => ({
  syncCostPrices: (...args: unknown[]) => syncCostPricesMock(...args),
}));
vi.mock("../api/absorptions", () => ({
  fetchStatsAbsorptions: (...args: unknown[]) => fetchStatsAbsorptionsMock(...args),
}));

// recharts is heavy and code-split behind `React.lazy`; the charts are strictly
// presentational and irrelevant to what this file asserts (the sync button lives in
// StatsModal precisely because StatsCharts has no access to the refetch).
//
// The stub DOES surface the filter contract (#819): the band lives in the
// charts, its state in the shell, so what this file can assert about the band is
// what crosses that seam — the values handed down, and what a change sends back.
vi.mock("./StatsCharts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./StatsCharts")>();
  const Stub = ({
    completedOnly,
    onCompletedOnlyChange,
    band,
    onBandChange,
    onResetFilters,
    onAbsorptionsChanged,
    uncombined,
    onUncombinedChange,
    showUncombined,
  }: {
    completedOnly: boolean;
    onCompletedOnlyChange: (value: boolean) => void;
    band: PerformanceBand;
    onBandChange: (band: PerformanceBand) => void;
    onResetFilters: () => void;
    onAbsorptionsChanged: () => void;
    uncombined: boolean;
    onUncombinedChange: (value: boolean) => void;
    showUncombined: boolean;
  }) => (
    <div
      data-testid="stats-charts-stub"
      data-completed-only={String(completedOnly)}
      data-uncombined={String(uncombined)}
      data-show-uncombined={String(showUncombined)}
      data-duration-mode={band.durationMode}
      data-zoom={band.zoom}
      data-axis={band.axis}
      data-node-kinds={band.nodeKinds.join(",")}
    >
      <button
        type="button"
        data-testid="stub-toggle-cohort"
        onClick={() => onCompletedOnlyChange(!completedOnly)}
      />
      <button
        type="button"
        data-testid="stub-waiting-mode"
        onClick={() => onBandChange({ ...band, durationMode: "waiting" })}
      />
      <button type="button" data-testid="stub-reset" onClick={onResetFilters} />
      <button type="button" data-testid="stub-combined" onClick={onAbsorptionsChanged} />
      <button
        type="button"
        data-testid="stub-toggle-uncombined"
        onClick={() => onUncombinedChange(!uncombined)}
      />
    </div>
  );
  return {
    default: (props: React.ComponentProps<typeof actual.default>) =>
      realCharts ? (
        <actual.default {...props} />
      ) : (
        <Stub {...(props as React.ComponentProps<typeof Stub>)} />
      ),
  };
});

import StatsModal from "./StatsModal";
import type { PerformanceBand } from "../model/statsFilters";
import type { StatsCost } from "../../shared/generated/StatsCost";
import type { StatsOverview } from "../../shared/generated/StatsOverview";
import type { SyncCostPricesReport } from "../model/prices";
import type { ExtensionEntry } from "../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../shared/generated/UiContribution";
import type { StatsTabProps } from "../../shared/extensions/contract";
import { hostReactMajor } from "../../shared/extensions/shared";
import {
  resetExtensions,
  useExtensionsStore,
  type BundleModule,
} from "../../shared/extensions/store";

// The charts are a `React.lazy` chunk: its first import in a file transforms a
// tree of modules, longer than the default 1 s of `findBy*` on a loaded machine.
const CHARTS_CHUNK = { timeout: 10_000 };

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
    by_model: [],
    infrastructure: [],
    waited_executions: 0,
    executions: 0,
  });
  syncCostPricesMock.mockReset().mockResolvedValue(report());
  fetchStatsAbsorptionsMock.mockReset().mockResolvedValue({ absorptions: [] });
  realCharts = false;
  localStorage.clear();
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
    it("covers the application and exposes Performance as the fifth tab of the strip", async () => {
      render(<StatsModal open onClose={() => {}} />);

      expect(await screen.findByTestId("stats-modal")).toHaveClass("h-[calc(100dvh/var(--app-zoom,1))]", "w-[calc(100vw/var(--app-zoom,1))]");
      const rail = screen.getByRole("tablist", { name: "Stats sections" });
      // A strip of tabs under the header, not a side rail.
      expect(rail).not.toHaveClass("flex-col");
      expect(rail.closest("[data-testid='stats-modal']")?.querySelector("header")).not.toContainElement(rail);
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
      // The charts are code-split behind `React.lazy` (#928: now a tree of
      // modules): wait for the chunk rather than assume an earlier test loaded it.
      expect(await screen.findByTestId("stats-charts-stub", {}, CHARTS_CHUNK)).toBeInTheDocument();
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

    // And the reload key never reaches the API: the call keeps its shape (period,
    // bucket, cohort, uncombined), which `useStats.test.ts` asserts exactly (Vitest
    // compares arity strictly). The modal owns the period, so assert the SHAPE, not
    // literal dates.
    const args = fetchStatsCostMock.mock.calls.at(-1)!;
    expect(args).toHaveLength(5);
    expect(args[2]).toBe("day"); // the 30d default preset's bucket
  });
});

describe("StatsModal — per-tab filters, ephemeral (#819)", () => {
  const stub = () => screen.getByTestId("stats-charts-stub");
  // `completed_only` is the 4th argument of all three fetchers.
  const cohortOf = (calls: unknown[][]) => calls.at(-1)![3];

  it("keeps only the period in the title bar", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);

    expect(await screen.findByTestId("stats-tab-runs")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Period" })).toBeInTheDocument();
    // The cohort left the bar for the tabs: it is per tab now.
    expect(screen.queryByTestId("stats-completed-only")).not.toBeInTheDocument();

    // Pricing details is the one section-specific button the bar still carries.
    await user.click(screen.getByTestId("stats-tab-cost"));
    expect(screen.getByTestId("stats-pricing-trigger")).toBeInTheDocument();
  });

  it("opens each tab on its own cohort, and a flip refetches that tab alone", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);

    // Overview: every run, so an error is never hidden by default.
    await waitFor(() => expect(fetchStatsOverviewMock).toHaveBeenCalled());
    expect(cohortOf(fetchStatsOverviewMock.mock.calls)).toBe(false);
    expect(await screen.findByTestId("stats-charts-stub", {}, CHARTS_CHUNK)).toHaveAttribute(
      "data-completed-only",
      "false",
    );

    // Cost: every run too — the spend of a failed run is still spend.
    await user.click(screen.getByTestId("stats-tab-cost"));
    await waitFor(() => expect(fetchStatsCostMock).toHaveBeenCalled());
    expect(cohortOf(fetchStatsCostMock.mock.calls)).toBe(false);

    // Performance: completed runs only, so durations are not polluted by runs
    // still in flight.
    await user.click(screen.getByTestId("stats-tab-performance"));
    await waitFor(() => expect(fetchStatsPerformanceMock).toHaveBeenCalled());
    expect(cohortOf(fetchStatsPerformanceMock.mock.calls)).toBe(true);
    expect(stub()).toHaveAttribute("data-completed-only", "true");

    // Flipping Performance's cohort refetches Performance — and nothing else.
    const costCalls = fetchStatsCostMock.mock.calls.length;
    const overviewCalls = fetchStatsOverviewMock.mock.calls.length;
    await user.click(screen.getByTestId("stub-toggle-cohort"));
    await waitFor(() =>
      expect(cohortOf(fetchStatsPerformanceMock.mock.calls)).toBe(false),
    );
    expect(fetchStatsCostMock).toHaveBeenCalledTimes(costCalls);
    expect(fetchStatsOverviewMock).toHaveBeenCalledTimes(overviewCalls);

    // …and Cost is where it was left.
    await user.click(screen.getByTestId("stats-tab-cost"));
    expect(stub()).toHaveAttribute("data-completed-only", "false");
  });

  it("shares one cohort between Overview, Sessions and Triggers", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);

    await user.click(await screen.findByTestId("stub-toggle-cohort"));
    await waitFor(() =>
      expect(cohortOf(fetchStatsOverviewMock.mock.calls)).toBe(true),
    );
    // The three tabs read the same response, so they read the same cohort.
    for (const tab of ["sessions", "triggers"]) {
      await user.click(screen.getByTestId(`stats-tab-${tab}`));
      expect(stub()).toHaveAttribute("data-completed-only", "true");
    }
  });

  it("opens Performance on Active, every kind, Full and independent scales", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    await user.click(await screen.findByTestId("stats-tab-performance"));

    expect(stub()).toHaveAttribute("data-duration-mode", "active");
    expect(stub()).toHaveAttribute("data-zoom", "full");
    expect(stub()).toHaveAttribute("data-axis", "independent");
    expect(stub()).toHaveAttribute(
      "data-node-kinds",
      "interactive,orchestrator,standard",
    );
  });

  it("keeps a change across tabs, and forgets it when Stats is closed and reopened", async () => {
    // The close/reopen cycle is driven through the `open` prop, as the app drives
    // it: this component stays mounted across opens (#717 keeps both full-window
    // siblings in the tree), so a surface holding its state behind `open={false}`
    // would hand the deviated band straight back — the bug this pins.
    const user = userEvent.setup();
    const onClose = () => {};
    const { rerender } = render(<StatsModal open onClose={onClose} />);
    await user.click(await screen.findByTestId("stats-tab-performance"));
    await user.click(screen.getByTestId("stub-waiting-mode"));
    await user.click(screen.getByTestId("stub-toggle-cohort"));
    await user.click(screen.getByTestId("stats-period-7d"));
    expect(stub()).toHaveAttribute("data-duration-mode", "waiting");
    await waitFor(() => expect(stub()).toHaveAttribute("data-completed-only", "false"));

    // A trip to another tab and back keeps it: the shell outlives the sections.
    await user.click(screen.getByTestId("stats-tab-cost"));
    await user.click(screen.getByTestId("stats-tab-performance"));
    expect(stub()).toHaveAttribute("data-duration-mode", "waiting");

    // Closing does not: the open is the lifetime of every Stats setting.
    rerender(<StatsModal open={false} onClose={onClose} />);
    expect(screen.queryByTestId("stats-modal")).not.toBeInTheDocument();

    rerender(<StatsModal open onClose={onClose} />);
    // Period back to 30 days, section back to the first one…
    expect(await screen.findByTestId("stats-period-30d")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByTestId("stats-period-7d")).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByTestId("stats-tab-runs")).toHaveAttribute("aria-selected", "true");
    // …and Performance back on its own defaults.
    await user.click(screen.getByTestId("stats-tab-performance"));
    expect(stub()).toHaveAttribute("data-duration-mode", "active");
    expect(stub()).toHaveAttribute("data-completed-only", "true");
    expect(stub()).toHaveAttribute("data-zoom", "full");
    expect(stub()).toHaveAttribute("data-axis", "independent");
    expect(localStorage.getItem("pdo.stats.completed_only")).toBeNull();
    expect(localStorage.getItem("pdo.stats.zoom")).toBeNull();
  });

  it("drops the pricing drawer and the entry tab of a programmatic open (#690)", async () => {
    // Settings › Diagnostics enters on Cost with the drawer open. That entry
    // belongs to the open that carries it: the same surface reopened bare lands
    // on the defaults like any other open.
    const onClose = () => {};
    const { rerender } = render(
      <StatsModal open onClose={onClose} initialTab="cost" initialPricingOpen />,
    );
    expect(await screen.findByTestId("stats-pricing-details")).toBeInTheDocument();
    expect(screen.getByTestId("stats-tab-cost")).toHaveAttribute("aria-selected", "true");

    rerender(<StatsModal open={false} onClose={onClose} initialTab="cost" initialPricingOpen />);
    rerender(<StatsModal open onClose={onClose} />);

    expect(await screen.findByTestId("stats-tab-runs")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
  });

  it("ignores a stale per-browser key an older build left behind", async () => {
    const user = userEvent.setup();
    localStorage.setItem("pdo.stats.completed_only", "false");
    localStorage.setItem("pdo.stats.exclude_user_wait", "false");
    localStorage.setItem("pdo.stats.node_kinds", JSON.stringify(["interactive"]));
    render(<StatsModal open onClose={() => {}} />);
    await user.click(await screen.findByTestId("stats-tab-performance"));

    expect(stub()).toHaveAttribute("data-completed-only", "true");
    expect(stub()).toHaveAttribute("data-duration-mode", "active");
    expect(stub()).toHaveAttribute(
      "data-node-kinds",
      "interactive,orchestrator,standard",
    );
  });

  it("puts the tab back on its defaults on reset", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    await user.click(await screen.findByTestId("stats-tab-performance"));
    await user.click(screen.getByTestId("stub-waiting-mode"));
    await user.click(screen.getByTestId("stub-toggle-cohort"));
    await waitFor(() =>
      expect(stub()).toHaveAttribute("data-completed-only", "false"),
    );

    await user.click(screen.getByTestId("stub-reset"));
    expect(stub()).toHaveAttribute("data-duration-mode", "active");
    expect(stub()).toHaveAttribute("data-completed-only", "true");
  });

  it("never marks the rail, whatever the band says", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    await user.click(await screen.findByTestId("stats-tab-performance"));
    await user.click(screen.getByTestId("stub-waiting-mode"));
    await user.click(screen.getByTestId("stub-toggle-cohort"));

    // The band is on screen and says exactly what it does: a deliberate reading
    // is not an anomaly to flag.
    expect(screen.getByTestId("stats-tab-performance")).not.toHaveAttribute(
      "data-dirty",
    );
    expect(
      screen.queryByTestId("stats-tab-performance-dirty"),
    ).not.toBeInTheDocument();
  });
});

describe("StatsModal — absorptions (#890)", () => {
  it("refetches the tab on screen after a Combine, without forcing Performance past its memo", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} initialTab="performance" />);
    await screen.findByTestId("stats-charts-stub", {}, CHARTS_CHUNK);
    await waitFor(() => expect(fetchStatsPerformanceMock).toHaveBeenCalledTimes(1));
    const overviewCalls = fetchStatsOverviewMock.mock.calls.length;

    await user.click(screen.getByTestId("stub-combined"));

    await waitFor(() => expect(fetchStatsPerformanceMock).toHaveBeenCalledTimes(2));
    // `refresh` stays false: the daemon keys its memo on the absorptions.
    expect(fetchStatsPerformanceMock.mock.calls[1][2]).toBe(false);
    expect(fetchStatsOverviewMock.mock.calls.length).toBe(overviewCalls + 1);
  });

  it("keeps the scrollbar gutter and paints no caret on the Stats pane", async () => {
    render(<StatsModal open onClose={() => {}} />);
    await screen.findByTestId("stats-charts-stub", {}, CHARTS_CHUNK);
    const main = screen.getByRole("main");
    expect(main.className).toContain("[scrollbar-gutter:stable]");
    expect(main.className).toContain("[caret-color:transparent]");
    expect(main.className).toContain("[&_input]:[caret-color:auto]");
  });
});

describe("StatsModal — « Uncombined » (#891)", () => {
  const ABSORPTION = {
    absorptions: [
      {
        dimension: "pipeline",
        scope: "",
        absorbent: { key: "digest-v2", name: "Digest v2" },
        members: [
          { key: "digest", name: "Digest", origin: "rename", created_at: "2026-09-20T10:00:00Z" },
        ],
      },
    ],
  };
  const uncombinedOf = (calls: unknown[][]) => calls.at(-1)!.at(-1);

  it("is on the Sessions, Triggers, Cost and Performance bands, never on Overview", async () => {
    realCharts = true;
    fetchStatsAbsorptionsMock.mockResolvedValue(ABSORPTION);
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);

    await screen.findByTestId("stats-filter-band");
    await waitFor(() => expect(fetchStatsAbsorptionsMock).toHaveBeenCalled());
    expect(screen.queryByTestId("stats-uncombined")).not.toBeInTheDocument();

    for (const tab of ["sessions", "triggers", "cost", "performance"]) {
      await user.click(screen.getByTestId(`stats-tab-${tab}`));
      const chip = await screen.findByTestId("stats-uncombined");
      expect(chip).toHaveAttribute("aria-checked", "false");
      expect(chip).toHaveTextContent("uncombined");
    }
    await user.click(screen.getByTestId("stats-tab-runs"));
    expect(screen.queryByTestId("stats-uncombined")).not.toBeInTheDocument();
  });

  it("is absent while the instance has no absorption", async () => {
    realCharts = true;
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    await waitFor(() => expect(fetchStatsAbsorptionsMock).toHaveBeenCalled());
    for (const tab of ["sessions", "triggers", "cost", "performance"]) {
      await user.click(screen.getByTestId(`stats-tab-${tab}`));
      await screen.findByTestId(
        tab === "performance" ? "stats-performance-filters" : "stats-filter-band",
      );
      expect(screen.queryByTestId("stats-uncombined")).not.toBeInTheDocument();
    }
  });

  it("reads every tab without the absorptions when on, and is off again at the next open", async () => {
    realCharts = true;
    fetchStatsAbsorptionsMock.mockResolvedValue(ABSORPTION);
    const user = userEvent.setup();
    const onClose = () => {};
    const { rerender } = render(<StatsModal open onClose={onClose} initialTab="sessions" />);

    await user.click(await screen.findByTestId("stats-uncombined"));
    await waitFor(() => expect(uncombinedOf(fetchStatsOverviewMock.mock.calls)).toBe(true));
    expect(screen.getByTestId("stats-uncombined")).toHaveAttribute("aria-checked", "true");

    // One reading for the four tabs: Cost and Performance fetch uncombined too.
    await user.click(screen.getByTestId("stats-tab-cost"));
    await waitFor(() => expect(uncombinedOf(fetchStatsCostMock.mock.calls)).toBe(true));
    expect(screen.getByTestId("stats-uncombined")).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByTestId("stats-tab-performance"));
    await waitFor(() =>
      expect(uncombinedOf(fetchStatsPerformanceMock.mock.calls)).toBe(true),
    );

    // Closing forgets it, like every Stats setting.
    rerender(<StatsModal open={false} onClose={onClose} />);
    rerender(<StatsModal open onClose={onClose} initialTab="sessions" />);
    const chip = await screen.findByTestId("stats-uncombined");
    expect(chip).toHaveAttribute("aria-checked", "false");
    expect(uncombinedOf(fetchStatsOverviewMock.mock.calls)).toBe(false);
  });

  it("re-reads the absorptions after a Combine, and keeps the chip while it is on", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} initialTab="sessions" />);
    const stub = await screen.findByTestId("stats-charts-stub", {}, CHARTS_CHUNK);
    await waitFor(() => expect(fetchStatsAbsorptionsMock).toHaveBeenCalledTimes(1));
    expect(stub).toHaveAttribute("data-show-uncombined", "false");

    fetchStatsAbsorptionsMock.mockResolvedValue(ABSORPTION);
    await user.click(screen.getByTestId("stub-combined"));
    await waitFor(() =>
      expect(screen.getByTestId("stats-charts-stub")).toHaveAttribute(
        "data-show-uncombined",
        "true",
      ),
    );

    await user.click(screen.getByTestId("stub-toggle-uncombined"));
    fetchStatsAbsorptionsMock.mockResolvedValue({ absorptions: [] });
    await user.click(screen.getByTestId("stub-combined"));
    await waitFor(() => expect(fetchStatsAbsorptionsMock).toHaveBeenCalledTimes(3));
    // No absorption left, but the reading is on: the chip stays to turn it off.
    expect(screen.getByTestId("stats-charts-stub")).toHaveAttribute(
      "data-show-uncombined",
      "true",
    );
  });
});

describe("StatsModal — Pricing details in the shell's secondary panel (#944)", () => {
  async function openPricing(onClose = vi.fn()) {
    const user = userEvent.setup();
    render(<StatsModal open onClose={onClose} />);
    await user.click(await screen.findByTestId("stats-tab-cost"));
    await user.click(screen.getByTestId("stats-pricing-trigger"));
    return { user, onClose };
  }

  it("closes on its ✕ and stays on Cost with the period untouched", async () => {
    const { user, onClose } = await openPricing();
    await user.click(screen.getByTestId("stats-period-7d"));
    const panel = screen.getByTestId("stats-pricing-details");
    expect(panel).toHaveTextContent("Esc returns to Stats");

    await user.click(within(panel).getByRole("button", { name: "Close panel" }));

    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("stats-tab-cost")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("stats-period-7d")).toHaveAttribute("aria-pressed", "true");
  });

  it("toggles from the header trigger, which exposes aria-expanded", async () => {
    const { user } = await openPricing();
    const trigger = screen.getByTestId("stats-pricing-trigger");
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("stats-pricing-details")).toBeInTheDocument();

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();

    await user.click(trigger);
    expect(screen.getByTestId("stats-pricing-details")).toBeInTheDocument();
  });

  it("keeps the header reachable while open: Refresh, the period and Close stats", async () => {
    const { user, onClose } = await openPricing();
    await waitFor(() => expect(fetchStatsCostMock).toHaveBeenCalledTimes(1));
    const panel = screen.getByTestId("stats-pricing-details");
    // Framed by the shell under the header, not over it.
    expect(panel).toHaveClass("top-(--h-topbar)");
    expect(panel).not.toHaveClass("inset-y-0");

    await user.click(screen.getByTestId("stats-refresh"));
    await waitFor(() => expect(fetchStatsCostMock).toHaveBeenCalledTimes(2));
    await user.click(screen.getByTestId("stats-period-7d"));
    expect(screen.getByTestId("stats-period-7d")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("stats-pricing-details")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Close stats" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close on a click in the main pane", async () => {
    const { user } = await openPricing();
    await user.click(screen.getByTestId("stats-charts-stub"));
    expect(screen.getByTestId("stats-pricing-details")).toBeInTheDocument();
  });

  it("keeps Sync costs in the panel header", async () => {
    await openPricing();
    const panel = screen.getByTestId("stats-pricing-details");
    const header = within(panel).getByRole("heading", { name: "Pricing details" }).parentElement!;
    expect(within(header).getByTestId("stats-sync-prices")).toHaveTextContent("Sync costs");
  });

  it("closes when leaving the Cost tab", async () => {
    const { user } = await openPricing();
    await user.click(screen.getByTestId("stats-tab-runs"));
    await user.click(screen.getByTestId("stats-tab-cost"));
    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
    expect(screen.getByTestId("stats-pricing-trigger")).toHaveAttribute("aria-expanded", "false");
  });

  it("shows the trigger's open state, not only aria-expanded", async () => {
    const { user } = await openPricing();
    const trigger = screen.getByTestId("stats-pricing-trigger");
    expect(trigger).toHaveAttribute("aria-pressed", "true");

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-pressed", "false");
  });

  it("returns focus to the trigger on ✕, Escape and the toggle", async () => {
    const { user } = await openPricing();
    const trigger = screen.getByTestId("stats-pricing-trigger");

    await user.click(screen.getByTestId("stats-pricing-details-close"));
    expect(trigger).toHaveFocus();

    await user.keyboard("{Enter}");
    expect(screen.getByTestId("stats-pricing-details")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    await user.click(trigger);
    await user.click(trigger);
    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("closes the programmatic entry (Settings › Diagnostics) on its ✕ too", async () => {
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} initialTab="cost" initialPricingOpen />);
    await user.click(await screen.findByTestId("stats-pricing-details-close"));
    expect(screen.queryByTestId("stats-pricing-details")).not.toBeInTheDocument();
    expect(screen.getByTestId("stats-tab-cost")).toHaveAttribute("aria-selected", "true");
  });
});

describe("StatsModal — the `stats/tab` location (extensions' tabs)", () => {
  function serveTabs(module: BundleModule, contributions: UiContribution[]) {
    const entry: ExtensionEntry = {
      id: "demo",
      name: "Demo",
      version: "1.0.0",
      ui: {
        bundle_url: "/ext/demo/ui.js",
        react_major: hostReactMajor(),
        ui_module_major: null,
        contributions,
        commands: [],
        locations: [],
      },
    };
    useExtensionsStore.setState({
      extensions: [{ entry, status: "ready", module }],
      started: true,
      loaded: true,
    });
  }
  const tab = (id: string, label: string, component: string) =>
    ({ location: "stats/tab", id, label, type: "tab", component }) as UiContribution;

  afterEach(() => resetExtensions());

  it("keeps the core's five tabs when no extension contributes one", async () => {
    resetExtensions();
    render(<StatsModal open onClose={() => {}} />);
    const rail = await screen.findByRole("tablist", { name: "Stats sections" });
    expect(within(rail).getAllByRole("tab").map((b) => b.textContent)).toEqual([
      "Runs",
      "Sessions",
      "Triggers",
      "Cost",
      "Performance",
    ]);
  });

  it("adds the extension's tab after Performance, opened in the same frame with the period", async () => {
    serveTabs(
      {
        Flow: ({ context, extension }: StatsTabProps) => (
          <p data-testid="flow">
            {`${extension.id} ${context.location} ${context.period.preset} ${context.period.bucket} ${context.refresh}`}
          </p>
        ),
      },
      [tab("flow", "Flow", "Flow")],
    );
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    const rail = await screen.findByRole("tablist", { name: "Stats sections" });
    await waitFor(() =>
      expect(within(rail).getAllByRole("tab").map((b) => b.textContent)).toEqual([
        "Runs",
        "Sessions",
        "Triggers",
        "Cost",
        "Performance",
        "Flow",
      ]),
    );
    await user.click(screen.getByTestId("stats-tab-demo:flow"));
    expect(screen.getByTestId("flow")).toHaveTextContent("demo stats/tab 30d day 0");
    expect(screen.queryByTestId("stats-charts-stub")).not.toBeInTheDocument();
    // The frame's header stays: the period and Refresh reach the tab.
    await user.click(screen.getByTestId("stats-period-90d"));
    expect(screen.getByTestId("flow")).toHaveTextContent("demo stats/tab 90d week 0");
    await waitFor(() => expect(screen.getByTestId("stats-refresh")).toBeEnabled());
    await user.click(screen.getByTestId("stats-refresh"));
    expect(screen.getByTestId("flow")).toHaveTextContent("90d week 1");
    // Back to a core tab.
    await user.click(screen.getByTestId("stats-tab-runs"));
    expect(await screen.findByTestId("stats-charts-stub", {}, CHARTS_CHUNK)).toBeInTheDocument();
    expect(screen.queryByTestId("flow")).not.toBeInTheDocument();
  });

  it("keeps a tab that throws to its own content", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    serveTabs(
      {
        Boom: () => {
          throw new Error("boom");
        },
      },
      [tab("boom", "Boom", "Boom")],
    );
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    await user.click(await screen.findByTestId("stats-tab-demo:boom"));
    expect(screen.getByTestId("extension-failed-demo")).toHaveTextContent(
      "This element of the extension demo failed.",
    );
    expect(screen.getByTestId("stats-modal")).toBeInTheDocument();
    await user.click(screen.getByTestId("stats-tab-cost"));
    expect(screen.getByTestId("stats-pricing-trigger")).toBeInTheDocument();
    vi.restoreAllMocks();
  });

  it("does not carry a failed tab's error over to the next extension tab", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    serveTabs(
      {
        Boom: () => {
          throw new Error("boom");
        },
        Flow: () => <p data-testid="flow">flow</p>,
      },
      [tab("boom", "Boom", "Boom"), tab("flow", "Flow", "Flow")],
    );
    const user = userEvent.setup();
    render(<StatsModal open onClose={() => {}} />);
    await user.click(await screen.findByTestId("stats-tab-demo:boom"));
    expect(screen.getByTestId("extension-failed-demo")).toBeInTheDocument();
    // Straight from the failed tab to its neighbour, no core tab in between.
    await user.click(screen.getByTestId("stats-tab-demo:flow"));
    expect(screen.getByTestId("flow")).toHaveTextContent("flow");
    expect(screen.queryByTestId("extension-failed-demo")).not.toBeInTheDocument();
    vi.restoreAllMocks();
  });
});
