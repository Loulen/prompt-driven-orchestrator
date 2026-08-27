import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { RotateCw, X } from "lucide-react";
import { syncCostPrices } from "../api";
import { useStats } from "../hooks/useStats";
import type { PriceRow, StatsCost, SyncCostPricesReport } from "../types";
import type { StatsTab } from "./StatsCharts";

const StatsCharts = lazy(() => import("./StatsCharts"));

interface Props {
  open: boolean;
  onClose: () => void;
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

export default function StatsModal({ open, onClose }: Props) {
  const [preset, setPreset] = useState<Preset>("30d");
  const [tab, setTab] = useState<StatsTab>("runs");
  const [pricingOpen, setPricingOpen] = useState(false);
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

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (
        document.querySelector(
          '[data-testid="tooltip-content"][data-state="delayed-open"], [data-testid="tooltip-content"][data-state="instant-open"]',
        )
      ) {
        return;
      }
      if (pricingOpen) setPricingOpen(false);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose, pricingOpen]);

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

  return (
    <div className="fixed inset-0 z-50 bg-bg-2">
      <div className="relative flex h-screen w-screen flex-col bg-bg-4" data-testid="stats-modal">
        <header className="flex min-h-14 items-center gap-4 border-b border-line px-4">
          <h2 className="font-semibold text-fg">Stats</h2>
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
          <div
            className="ml-auto text-fg-4"
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
          <button
            type="button"
            onClick={onClose}
            aria-label="Close stats"
            className="grid h-7 w-7 place-items-center rounded text-fg-3 hover:bg-bg-5"
          >
            <X size={15} />
          </button>
        </header>

        <div className="flex min-h-0 flex-1">
          <nav
            className="flex w-36 shrink-0 flex-col gap-1 border-r border-line bg-bg-3 p-3"
            role="tablist"
            aria-label="Stats sections"
            onKeyDown={(event) => {
              if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
              event.preventDefault();
              const index = TABS.findIndex((item) => item.id === tab);
              const delta = event.key === "ArrowDown" ? 1 : -1;
              const next = TABS[(index + delta + TABS.length) % TABS.length];
              setTab(next.id);
              document.querySelector<HTMLElement>(`[data-testid='stats-tab-${next.id}']`)?.focus();
            }}
          >
            {TABS.map((item) => (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={tab === item.id}
                tabIndex={tab === item.id ? 0 : -1}
                data-testid={`stats-tab-${item.id}`}
                onClick={() => {
                  setTab(item.id);
                  if (item.id !== "cost") setPricingOpen(false);
                }}
                className={`rounded px-3 py-2 text-left ${
                  tab === item.id ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-4"
                }`}
              >
                {item.label}
              </button>
            ))}
          </nav>

          <main className={`min-w-0 flex-1 overflow-y-auto p-5 ${refreshing ? "opacity-65" : ""}`}>
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
          </main>
        </div>

        {pricingOpen && (
          <PricingDetails
            cost={cost}
            syncing={syncing}
            syncError={syncError}
            syncReport={syncReport}
            onSync={onSyncPrices}
          />
        )}
      </div>
    </div>
  );
}
