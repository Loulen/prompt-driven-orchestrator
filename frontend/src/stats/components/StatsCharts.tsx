import { useTheme } from "../../hooks/useTheme";
import type { StatsCost } from "../../shared/generated/StatsCost";
import type { StatsOverview } from "../../shared/generated/StatsOverview";
import type { StatsPerformance } from "../../shared/generated/StatsPerformance";
import {
  DEFAULT_PERFORMANCE_BAND,
  type PerformanceBand,
} from "../model/statsFilters";
import { CohortLine, EmptyNote } from "./charts/common/ChartPrimitives";
import { CostTab } from "./charts/cost/CostTab";
import { CohortOnlyBand } from "./charts/filters/FilterBand";
import { RunsTab } from "./charts/overview/RunsTab";
import { SessionsTab } from "./charts/overview/SessionsTab";
import { TriggersTab } from "./charts/overview/TriggersTab";
import { PerformanceTab } from "./charts/performance/PerformanceTab";

export type StatsTab =
  "runs" | "sessions" | "triggers" | "cost" | "performance";

export interface StatsChartsProps {
  tab: StatsTab;
  overview: StatsOverview | null;
  cost: StatsCost | null;
  costError: string | null;
  performance?: StatsPerformance | null;
  performanceError?: string | null;
  /** The cohort of the tab on screen (#819) — the chip in its band, and the
   *  cohort line under it. The narrowing itself happened in the daemon. */
  completedOnly?: boolean;
  onCompletedOnlyChange?: (value: boolean) => void;
  /** The Performance band's other controls, owned by the shell so a trip to
   *  another tab keeps them (#819). Defaulted so a caller that does not care
   *  gets the surface's own defaults. */
  band?: PerformanceBand;
  onBandChange?: (band: PerformanceBand) => void;
  onResetFilters?: () => void;
  /** A Combine or an Uncombine landed (#890): the host refetches every tab. */
  onAbsorptionsChanged?: () => void;
  /** « Uncombined » (#891): the rows as the event log wrote them. Owned by the
   *  shell (one reading for Sessions, Triggers, Cost and Performance, back to
   *  off at each open); the chip only shows when `showUncombined`. */
  uncombined?: boolean;
  onUncombinedChange?: (value: boolean) => void;
  showUncombined?: boolean;
}

/** The legend each tab's band carries — it names the tab, not the endpoint. */
const TAB_BAND_LABEL: Record<Exclude<StatsTab, "performance">, string> = {
  runs: "Overview filters",
  sessions: "Sessions filters",
  triggers: "Triggers filters",
  cost: "Cost filters",
};

export default function StatsCharts({
  tab,
  overview,
  cost,
  costError,
  performance = null,
  performanceError = null,
  completedOnly = false,
  onCompletedOnlyChange = () => {},
  band = DEFAULT_PERFORMANCE_BAND,
  onBandChange = () => {},
  onResetFilters = () => {},
  onAbsorptionsChanged = () => {},
  uncombined = false,
  onUncombinedChange = () => {},
  showUncombined = false,
}: StatsChartsProps) {
  // #759: subscribing here re-renders the whole chart subtree on a theme switch,
  // so every `CHART.*` / `harnessColor()` read below resolves against the new
  // palette. Recharts bakes colours into props; nothing re-themes on its own.
  useTheme();

  if (tab === "performance") {
    // Performance places its band and its cohort line itself — the band above
    // the whole pane, the line under its head cards.
    return (
      <PerformanceTab
        performance={performance}
        error={performanceError}
        completedOnly={completedOnly}
        onCompletedOnlyChange={onCompletedOnlyChange}
        band={band}
        onBandChange={onBandChange}
        onResetFilters={onResetFilters}
        uncombined={uncombined}
        onUncombinedChange={onUncombinedChange}
        showUncombined={showUncombined}
        onAbsorptionsChanged={onAbsorptionsChanged}
      />
    );
  }
  const body = () => {
    if (tab === "cost")
      return (
        <CostTab
          cost={cost}
          error={costError}
          uncombined={uncombined}
          onAbsorptionsChanged={onAbsorptionsChanged}
        />
      );
    if (!overview) return <EmptyNote>Loading…</EmptyNote>;
    if (tab === "runs")
      return <RunsTab overview={overview} completedOnly={completedOnly} />;
    if (tab === "sessions")
      return (
        <SessionsTab
          overview={overview}
          uncombined={uncombined}
          onAbsorptionsChanged={onAbsorptionsChanged}
        />
      );
    return <TriggersTab overview={overview} />;
  };
  return (
    <>
      <div className="flex min-h-full flex-col gap-3">
        <CohortOnlyBand
          label={TAB_BAND_LABEL[tab]}
          completedOnly={completedOnly}
          onCompletedOnlyChange={onCompletedOnlyChange}
          uncombined={
            // Overview (the Runs tab) has no Pipeline rows: nothing to uncombine.
            tab !== "runs" && showUncombined
              ? { checked: uncombined, onChange: onUncombinedChange }
              : undefined
          }
        />
        <CohortLine
          completedOnly={completedOnly}
          note={
            // Trigger fires are not Runs: the cohort selects which Runs are
            // counted, and says so rather than implying it filtered the fires.
            tab === "triggers"
              ? "trigger fires are not filtered by run status"
              : undefined
          }
        />
        <div className="min-h-0 flex-1">{body()}</div>
      </div>
    </>
  );
}
