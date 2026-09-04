import { lazy, Suspense, useMemo, useState } from "react";
import { RotateCw } from "lucide-react";
import FullWindowShell from "./FullWindowShell";
import { syncCostPrices } from "../api";
import { useStats } from "../hooks/useStats";
import type { PriceRow, StatsCost, SyncCostPricesReport } from "../types";
import type { StatsTab } from "./StatsCharts";

const StatsCharts = lazy(() => import("./StatsCharts"));

interface Props {
  open: boolean;
  onClose: () => void;
  /**
   * Programmatic entry (#690): the tab to land on and whether the pricing drawer opens
   * with it — Settings › Diagnostics links to Cost › Pricing details. Read once at mount,
   * so the host bumps the component `key` when it wants them applied.
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

export default function StatsModal({
  open,
  onClose,
  initialTab = "runs",
  initialPricingOpen = false,
}: Props) {
  const [preset, setPreset] = useState<Preset>("30d");
  const [tab, setTab] = useState<StatsTab>(initialTab);
  const [pricingOpen, setPricingOpen] = useState(initialPricingOpen && initialTab === "cost");
  const [reloadKey, setReloadKey] = useState(0);
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
    open,
    period.from,
    period.to,
    period.bucket,
    tab === "cost",
    tab === "performance",
    reloadKey,
  );

  if (!open) return null;

  const refreshing =
    overviewReloadKey !== reloadKey ||
    (tab === "cost" && costReloadKey !== reloadKey) ||
    (tab === "performance" && performanceReloadKey !== reloadKey);

  const refresh = () => {
    setReloadKey((value) => value + 1);
  };

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

  // Escape order (Stats behaviour, kept by the shell contract): drawer first, then Stats.
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
      rail={TABS}
      activeRail={tab}
      onRailChange={(id) => {
        setTab(id as StatsTab);
        if (id !== "cost") setPricingOpen(false);
      }}
      railAriaLabel="Stats sections"
      railTestIdPrefix="stats-tab"
      mainClassName={`min-w-0 flex-1 overflow-y-auto p-5 ${refreshing ? "opacity-65" : ""}`}
      headerExtras={
        <div className="flex items-center gap-1" role="group" aria-label="Period">
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
        />
      </Suspense>
    </FullWindowShell>
  );
}
