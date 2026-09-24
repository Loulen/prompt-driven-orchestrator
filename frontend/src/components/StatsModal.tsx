import { lazy, Suspense, useMemo, useState } from "react";
import { RotateCw } from "lucide-react";
import FullWindowShell from "./FullWindowShell";
import { syncCostPrices } from "../api";
import { useStats } from "../hooks/useStats";
import type { PriceRow, StatsCost, SyncCostPricesReport } from "../types";
import type { StatsTab } from "./StatsCharts";
import {
  DEFAULT_PERFORMANCE_BAND,
  TAB_COHORT_DEFAULTS,
  type PerformanceBand,
} from "../lib/statsFilters";

const StatsCharts = lazy(() => import("./StatsCharts"));

interface Props {
  open: boolean;
  onClose: () => void;
  /**
   * Programmatic entry (#690): the tab to land on and whether the pricing drawer opens
   * with it — Settings › Diagnostics links to Cost › Pricing details. Read once at mount
   * — which is each open (#819) — so a host wanting them applied to a Stats already on
   * screen bumps the component `key`.
   */
  initialTab?: StatsTab;
  initialPricingOpen?: boolean;
}

type Preset = "7d" | "30d" | "90d" | "all";

const PRESETS: { id: Preset; label: string }[] = [
  { id: "7d", label: "7 days" },
  { id: "30d", label: "30 days" },
  { id: "90d", label: "90 days" },
  { id: "all", label: "All time" },
];

const TABS: { id: StatsTab; label: string }[] = [
  { id: "runs", label: "Runs" },
  { id: "sessions", label: "Sessions" },
  { id: "triggers", label: "Triggers" },
  { id: "cost", label: "Cost" },
  { id: "performance", label: "Performance" },
];

function utcDayStart(date: Date): string {
  return new Date(
    Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()),
  ).toISOString();
}

function presetPeriod(preset: Preset) {
  const now = new Date();
  const tomorrow = new Date(now);
  tomorrow.setUTCDate(tomorrow.getUTCDate() + 1);
  const daysAgo = (days: number) => {
    const date = new Date(now);
    date.setUTCDate(date.getUTCDate() - days);
    return utcDayStart(date);
  };

  if (preset === "7d") return { from: daysAgo(6), to: utcDayStart(tomorrow), bucket: "day" };
  if (preset === "30d") return { from: daysAgo(29), to: utcDayStart(tomorrow), bucket: "day" };
  if (preset === "90d") return { from: daysAgo(89), to: utcDayStart(tomorrow), bucket: "week" };
  return {
    from: "1970-01-01T00:00:00.000Z",
    to: utcDayStart(tomorrow),
    bucket: "month",
  };
}

function PriceRows({ rows }: { rows: PriceRow[] }) {
  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((row) => (
        <div
          key={row.key}
          className="flex items-center justify-between rounded bg-bg-3 px-2 py-1 font-mono text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <span>{row.key}</span>
          <span>
            <span className="mr-2 text-fg-4">{row.tier}</span>
            <span className="text-fg-2">${row.input}/${row.output} /MTok</span>
          </span>
        </div>
      ))}
    </div>
  );
}

function SyncResult({ report }: { report: SyncCostPricesReport | null }) {
  if (!report) return null;
  if (report.noop) {
    return (
      <div className="text-fg-4" data-testid="stats-sync-noop">
        {report.reason ?? "Price table already up to date."}
      </div>
    );
  }
  return (
    <div
      className="rounded-md border border-st-await/40 bg-st-await/10 px-3 py-2 text-fg-2"
      data-testid="stats-sync-report"
    >
      <div>
        {report.rows} price row(s) from <span className="font-mono">{report.source}</span>
        {report.fetched_at ? ` at ${report.fetched_at}` : ""}.
      </div>
      <ul className="list-disc pl-4">
        {report.added.length > 0 && <li>Newly priced: {report.added.join(", ")}</li>}
        {report.updated.length > 0 && <li>Price changed: {report.updated.join(", ")}</li>}
        {report.shadowed_by_manual.length > 0 && (
          <li>
            Kept from your <span className="font-mono">models.yaml</span> (overrides the fetched
            price): {report.shadowed_by_manual.join(", ")}
          </li>
        )}
        {report.rejected.length > 0 && <li>Refused by the source: {report.rejected.join("; ")}</li>}
      </ul>
    </div>
  );
}

function PricingDetails({
  cost,
  syncing,
  syncError,
  syncReport,
  onSync,
}: {
  cost: StatsCost | null;
  syncing: boolean;
  syncError: string | null;
  syncReport: SyncCostPricesReport | null;
  onSync: () => void;
}) {
  const warnings = cost
    ? [...cost.total.unpriced_models, ...cost.total.missing_reasons]
    : [];
  return (
    <aside
      className="absolute inset-y-0 right-0 z-20 w-[min(420px,90vw)] overflow-y-auto border-l border-line bg-bg-4 p-4 shadow-2xl"
      data-testid="stats-pricing-details"
    >
      <div className="mb-4 flex items-center justify-between">
        <h3 className="font-semibold text-fg">Pricing details</h3>
        <button
          type="button"
          onClick={onSync}
          disabled={syncing}
          data-testid="stats-sync-prices"
          className="rounded-md border border-line-strong bg-bg-3 px-2 py-1 text-fg-2 disabled:opacity-40"
        >
          {syncing ? "Syncing…" : "Sync costs"}
        </button>
      </div>
      <div className="flex flex-col gap-3" style={{ fontSize: "10.5px" }}>
        {syncError && (
          <div
            className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
            data-testid="stats-sync-error"
          >
            {syncError}
          </div>
        )}
        <SyncResult report={syncReport} />
        {warnings.map((warning) => (
          <div key={warning} className="text-st-await">
            {warning}
          </div>
        ))}
        {cost?.resolved.length ? (
          <PriceRows rows={cost.resolved} />
        ) : (
          <div className="text-fg-4">No resolved prices.</div>
        )}
      </div>
    </aside>
  );
}

/**
 * **Réglages de Stats éphémères** (#819, CONTEXT.md): a close forgets every
 * setting, an open rebuilds them. The settings live in plain `useState` on the
 * surface below, so the open IS their lifetime — this wrapper renders nothing
 * while Stats is closed, React unmounts the surface, and the next open starts
 * from the literals again (period, per-tab cohorts, band, section).
 *
 * The lifetime belongs here and not to the host: the app keeps this component
 * mounted across opens (#717 wants both full-window siblings in the tree with
 * stable keys), so a surface that held its state while `open` was false carried
 * a deviated band back into the next open — the whole rule, silently undone by
 * a detail of the host's tree.
 */
export default function StatsModal({ open, ...rest }: Props) {
  if (!open) return null;
  return <StatsSurface {...rest} />;
}

function StatsSurface({
  onClose,
  initialTab = "runs",
  initialPricingOpen = false,
}: Omit<Props, "open">) {
  const [preset, setPreset] = useState<Preset>("30d");
  const [tab, setTab] = useState<StatsTab>(initialTab);
  // #819 — the filter state, plain `useState` on literal defaults: nothing is
  // read from or written to this browser, so every mount of the surface opens
  // on the defaults (« Réglages de Stats éphémères »). The shell owns it
  // because the cohorts feed the fetches, which `StatsCharts` cannot reach;
  // switching tabs keeps it, since the shell outlives the sections.
  const [overviewCompletedOnly, setOverviewCompletedOnly] = useState<boolean>(
    TAB_COHORT_DEFAULTS.overview,
  );
  const [costCompletedOnly, setCostCompletedOnly] = useState<boolean>(
    TAB_COHORT_DEFAULTS.cost,
  );
  const [performanceCompletedOnly, setPerformanceCompletedOnly] =
    useState<boolean>(TAB_COHORT_DEFAULTS.performance);
  const [band, setBand] = useState<PerformanceBand>(DEFAULT_PERFORMANCE_BAND);
  const [pricingOpen, setPricingOpen] = useState(
    initialPricingOpen && initialTab === "cost",
  );
  const [reloadKey, setReloadKey] = useState(0);
  // Bumped by every Combine / Uncombine (#890): all tabs read the new absorptions.
  const [absorptionsVersion, setAbsorptionsVersion] = useState(0);
  const [syncReport, setSyncReport] = useState<SyncCostPricesReport | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncError, setSyncError] = useState<string | null>(null);
  const period = useMemo(() => presetPeriod(preset), [preset]);

  const {
    overview,
    cost,
    error,
    costError,
    performance,
    performanceError,
    computedAt,
    overviewReloadKey,
    costReloadKey,
    performanceReloadKey,
  } = useStats(
    // The surface only exists while Stats is open, so the hook's gate is simply
    // its mount: no fetch is ever armed behind a closed window.
    true,
    period.from,
    period.to,
    period.bucket,
    tab === "cost",
    tab === "performance",
    reloadKey,
    {
      overview: overviewCompletedOnly,
      cost: costCompletedOnly,
      performance: performanceCompletedOnly,
    },
    absorptionsVersion,
  );

  const refreshing =
    overviewReloadKey !== reloadKey ||
    (tab === "cost" && costReloadKey !== reloadKey) ||
    (tab === "performance" && performanceReloadKey !== reloadKey);

  const refresh = () => {
    setReloadKey((value) => value + 1);
  };

  // The cohort of the section on screen (#819). Overview, Sessions and Triggers
  // read one response, so they read and write one state; Cost and Performance
  // each own theirs, and flipping one leaves the others exactly where they are.
  const completedOnly =
    tab === "cost"
      ? costCompletedOnly
      : tab === "performance"
        ? performanceCompletedOnly
        : overviewCompletedOnly;
  const onCompletedOnlyChange = (value: boolean) => {
    if (tab === "cost") setCostCompletedOnly(value);
    else if (tab === "performance") setPerformanceCompletedOnly(value);
    else setOverviewCompletedOnly(value);
  };
  // « reset filters » puts the tab back on what it opens with — cohort, mode,
  // kinds, zoom and scales. The grouping and the sort are not filters and are
  // left alone.
  const onResetFilters = () => {
    setPerformanceCompletedOnly(TAB_COHORT_DEFAULTS.performance);
    setBand(DEFAULT_PERFORMANCE_BAND);
  };

  // No rail badge, whatever the band says (#819): the band is on screen, and a
  // deliberate reading is not an anomaly to flag.
  const rail = TABS;

  const onSyncPrices = async () => {
    setSyncing(true);
    setSyncError(null);
    setSyncReport(null);
    try {
      const report = await syncCostPrices();
      setSyncReport(report);
      setReloadKey((value) => value + 1);
    } catch (cause) {
      setSyncError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSyncing(false);
    }
  };

  // Escape order (Stats behaviour, kept by the shell contract): an open absorption
  // modal, then the row selection (both consumed by the tab, #890), then the
  // drawer, then Stats.
  const onEscape = () => {
    if (pricingOpen) setPricingOpen(false);
    else onClose();
  };

  return (
    <FullWindowShell
      title="Stats"
      testId="stats-modal"
      onClose={onClose}
      onEscape={onEscape}
      closeLabel="Close stats"
      rail={rail}
      activeRail={tab}
      onRailChange={(id) => {
        setTab(id as StatsTab);
        if (id !== "cost") setPricingOpen(false);
      }}
      railAriaLabel="Stats sections"
      railTestIdPrefix="stats-tab"
      // `scrollbar-gutter:stable` (#890): a Combine that removes rows must not
      // widen the pane — one scrollbar less made the charts reflow and print
      // x-axis labels they had been hiding. `caret-color:transparent`: nothing
      // in Stats is editable, so Chrome's caret browsing paints no blinking
      // caret on a clicked label; inputs and textareas keep theirs.
      mainClassName={`min-w-0 flex-1 overflow-y-auto p-5 [scrollbar-gutter:stable] [caret-color:transparent] [&_input]:[caret-color:auto] [&_textarea]:[caret-color:auto] ${refreshing ? "opacity-65" : ""}`}
      headerExtras={
        // #819 — the period is the only global setting left in the bar: every
        // other filter belongs to the tab it changes, in that tab's band.
        <div className="flex items-center gap-1">
          <div
            className="flex items-center gap-1"
            role="group"
            aria-label="Period"
          >
            {PRESETS.map((item) => (
              <button
                key={item.id}
                type="button"
                aria-pressed={preset === item.id}
                data-testid={`stats-period-${item.id}`}
                onClick={() => setPreset(item.id)}
                className={`rounded-md border px-2.5 py-1 ${
                  preset === item.id
                    ? "border-acc bg-acc/15 text-fg"
                    : "border-line-strong bg-bg-3 text-fg-2"
                }`}
                style={{ fontSize: "11px" }}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>
      }
      headerActions={
        <>
          <div
            className="text-fg-4"
            style={{ fontSize: "10.5px" }}
            data-testid="stats-computed-at"
          >
            {computedAt
              ? `Computed ${computedAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
              : "Computing…"}
          </div>
          <button
            type="button"
            onClick={refresh}
            disabled={refreshing}
            data-testid="stats-refresh"
            className="flex items-center gap-1.5 rounded border border-line bg-bg-3 px-2 py-1 text-fg-2 disabled:opacity-60"
          >
            <RotateCw size={12} className={refreshing ? "animate-spin" : ""} />
            Refresh
          </button>
          {tab === "cost" && (
            <button
              type="button"
              onClick={() => setPricingOpen(true)}
              data-testid="stats-pricing-trigger"
              className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
            >
              Pricing details
              {cost && cost.total.unpriced_models.length + cost.total.missing_reasons.length > 0
                ? ` (${cost.total.unpriced_models.length + cost.total.missing_reasons.length})`
                : ""}
            </button>
          )}
        </>
      }
      drawer={
        pricingOpen ? (
          <PricingDetails
            cost={cost}
            syncing={syncing}
            syncError={syncError}
            syncReport={syncReport}
            onSync={onSyncPrices}
          />
        ) : null
      }
    >
      {error && (
        <div
          className="mb-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
          data-testid="stats-error"
        >
          {error}
        </div>
      )}
      <Suspense
        fallback={
          <div
            className="min-h-[220px] px-1 py-8 text-center text-fg-4"
            data-testid="stats-charts-loading"
          >
            Loading charts…
          </div>
        }
      >
        <StatsCharts
          tab={tab}
          overview={overview}
          cost={cost}
          costError={costError}
          performance={performance}
          performanceError={performanceError}
          completedOnly={completedOnly}
          onCompletedOnlyChange={onCompletedOnlyChange}
          band={band}
          onBandChange={setBand}
          onResetFilters={onResetFilters}
          onAbsorptionsChanged={() => setAbsorptionsVersion((value) => value + 1)}
        />
      </Suspense>
    </FullWindowShell>
  );
}
