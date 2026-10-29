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

/** What a tab draws around its content: the filter bar (its drill path on the
 *  left, its own controls before the cohort) and the cohort line. */
export interface TabFrame {
  band: (crumbs?: React.ReactNode, controls?: React.ReactNode) => React.ReactNode;
  cohort: React.ReactNode;
}

export interface StatsChartsProps {
  tab: StatsTab;
  overview: StatsOverview | null;
  /** The overview request failed: Overview, Sessions and Triggers say so in
   *  their page instead of loading forever. */
  overviewError?: string | null;
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
  overviewError = null,
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
  // The bar and the cohort line every tab but Performance opens with (#819):
  // the tab draws them, so the bar can carry its drill path on the left.
  const frame: TabFrame = {
    band: (crumbs, controls) => (
      <CohortOnlyBand
        label={TAB_BAND_LABEL[tab]}
        completedOnly={completedOnly}
        onCompletedOnlyChange={onCompletedOnlyChange}
        crumbs={crumbs}
        uncombined={
          // Overview (the Runs tab) has no Pipeline rows: nothing to uncombine.
          tab !== "runs" && showUncombined
            ? { checked: uncombined, onChange: onUncombinedChange }
            : undefined
        }
      >
        {controls}
      </CohortOnlyBand>
    ),
    cohort: (
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
    ),
  };
  if (tab === "cost")
    return (
      <CostTab
        cost={cost}
        error={costError}
        uncombined={uncombined}
        onAbsorptionsChanged={onAbsorptionsChanged}
        frame={frame}
      />
    );
  if (tab === "sessions" && overview)
    return (
      <SessionsTab
        overview={overview}
        uncombined={uncombined}
        onAbsorptionsChanged={onAbsorptionsChanged}
        frame={frame}
      />
    );
  return (
    <div className="sx-tab">
      {frame.band()}
      <div className="sx-page">
        {frame.cohort}
        {!overview ? (
          overviewError ? (
            <div className="sx-err" role="alert" data-testid="stats-error">
              {overviewError}
            </div>
          ) : (
            <EmptyNote>Loading…</EmptyNote>
          )
        ) : tab === "runs" ? (
          <RunsTab overview={overview} completedOnly={completedOnly} />
        ) : (
          <TriggersTab overview={overview} />
        )}
      </div>
    </div>
  );
}
