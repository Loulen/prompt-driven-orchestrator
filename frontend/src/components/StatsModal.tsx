import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import { useStats } from "../hooks/useStats";
import { syncCostPrices } from "../api";
import type { SyncCostPricesReport } from "../types";
import type { StatsTab } from "./StatsCharts";

// First `React.lazy` in the repo (#377): recharts is heavy, so StatsCharts (its
// only importer) is code-split and loads when the modal first opens.
const StatsCharts = lazy(() => import("./StatsCharts"));

interface Props {
  open: boolean;
  onClose: () => void;
}

type Preset = "7d" | "30d" | "90d" | "all";

interface Period {
  from: string;
  to: string;
  bucket: string;
}

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
];

/** UTC midnight of `d`, as an ISO-Z string — aligns with the daemon's UTC `ts`
 *  and its `strftime` day bucketing. */
function utcDayStart(d: Date): string {
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate())).toISOString();
}

/**
 * Resolve a preset to a half-open `[from, to)` window + a bucket granularity —
 * pure client-side view state (invariant #7: NOT `instance_config`). `to` is the
 * start of tomorrow (UTC) so all of today is included.
 */
function presetPeriod(preset: Preset): Period {
  const now = new Date();
  const tomorrow = new Date(now);
  tomorrow.setUTCDate(tomorrow.getUTCDate() + 1);
  const to = utcDayStart(tomorrow);

  const daysAgo = (n: number): string => {
    const d = new Date(now);
    d.setUTCDate(d.getUTCDate() - n);
    return utcDayStart(d);
  };

  switch (preset) {
    case "7d":
      return { from: daysAgo(6), to, bucket: "day" };
    case "30d":
      return { from: daysAgo(29), to, bucket: "day" };
    case "90d":
      return { from: daysAgo(89), to, bucket: "week" };
    case "all":
      return { from: "1970-01-01T00:00:00.000Z", to, bucket: "month" };
  }
}

/**
 * Instance stats cockpit (#377, ADR-0029): a sister of SettingsModal, opened from
 * the TopBar. Cross-run aggregates filterable by period — runs/errors, node
 * sessions, trigger fires, and estimated cost (framed honestly). Two-endpoint
 * split: the overview loads on open; the cost tab lazily fetches `/stats/cost`
 * and code-splits recharts on first view.
 */
export default function StatsModal({ open, onClose }: Props) {
  const [preset, setPreset] = useState<Preset>("30d");
  const [tab, setTab] = useState<StatsTab>("runs");
  const period = useMemo(() => presetPeriod(preset), [preset]);

  // Price sync (#427, ADR-0034). The triplet mirrors
  // `guardTest`/`guardTesting`/`guardTestError` in TriggerDetailPanel — there is no
  // global toast system in this app (the one mention of the word is a "no toast"
  // comment), so every component renders its own message inline.
  const [syncReport, setSyncReport] = useState<SyncCostPricesReport | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncError, setSyncError] = useState<string | null>(null);
  // Bumped after a successful sync so `useStats` refetches `/stats/cost`. Without
  // it the numbers would not move until the period changed: the button would have
  // repaired the table and lied about it.
  const [reloadKey, setReloadKey] = useState(0);

  const { overview, cost, error, costError } = useStats(
    open,
    period.from,
    period.to,
    period.bucket,
    tab === "cost",
    reloadKey,
  );

  const onSyncPrices = async () => {
    setSyncing(true);
    setSyncError(null);
    setSyncReport(null);
    try {
      const report = await syncCostPrices();
      setSyncReport(report);
      // Even a noop bumps: the user asked, and a refetch is the honest confirmation
      // that what they see is current.
      setReloadKey((k) => k + 1);
    } catch (e) {
      setSyncError(e instanceof Error ? e.message : String(e));
    } finally {
      setSyncing(false);
    }
  };

  // Escape-to-close (grafted from MarkdownArtifactModal — SettingsModal lacks it).
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="flex max-h-[85vh] w-[720px] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="stats-modal"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            Stats
          </h2>
          <button
            onClick={onClose}
            aria-label="Close stats"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        {/* Period picker + tabs */}
        <div className="flex flex-col gap-2 border-b border-line px-4 py-3">
          <div className="flex items-center gap-1" role="group" aria-label="Period">
            {PRESETS.map((p) => (
              <button
                key={p.id}
                type="button"
                aria-pressed={preset === p.id}
                data-testid={`stats-period-${p.id}`}
                onClick={() => setPreset(p.id)}
                className={`rounded-md border px-2.5 py-1 transition-colors ${
                  preset === p.id
                    ? "border-acc bg-acc/15 text-fg"
                    : "border-line-strong bg-bg-3 text-fg-2 hover:bg-bg-4"
                }`}
                style={{ fontSize: "11px" }}
              >
                {p.label}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-1" role="tablist" aria-label="Stats sections">
            {TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={tab === t.id}
                data-testid={`stats-tab-${t.id}`}
                onClick={() => setTab(t.id)}
                className={`rounded-md px-2.5 py-1 transition-colors ${
                  tab === t.id ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-3 hover:text-fg-2"
                }`}
                style={{ fontSize: "11.5px" }}
              >
                {t.label}
              </button>
            ))}
            {/* Price sync (#427). Only on the Cost tab — the one place the table's
                staleness is visible — and pushed right with `ml-auto`. It lives here
                rather than in StatsCharts because that component is lazy and strictly
                presentational, with no access to the refetch; StatsModal owns
                `useStats`. Style copied from TriggerDetailPanel's "Test guard". */}
            {tab === "cost" && (
              <button
                type="button"
                onClick={onSyncPrices}
                disabled={syncing}
                data-testid="stats-sync-prices"
                title="Fetch public model prices and refresh the local price table"
                className="ml-auto flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-4 disabled:opacity-40"
                style={{ fontSize: "10.5px" }}
              >
                {syncing ? "Syncing…" : "Sync costs"}
              </button>
            )}
          </div>
        </div>

        {/* Body */}
        <div className="min-h-[260px] flex-1 overflow-y-auto px-4 py-4">
          {error && (
            <div
              className="mb-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
              style={{ fontSize: "11.5px" }}
              data-testid="stats-error"
            >
              {error}
            </div>
          )}
          {/* #427: a failed sync NAMES the source (the daemon answers 502 with the
              URL) — ADR-0030's rule that an explicitly requested effect which fails
              is a hard error, never a silent fallback. */}
          {syncError && (
            <div
              className="mb-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
              style={{ fontSize: "11.5px" }}
              data-testid="stats-sync-error"
            >
              {syncError}
            </div>
          )}
          {syncReport?.noop && (
            <div
              className="mb-3 text-fg-4"
              style={{ fontSize: "10.5px" }}
              data-testid="stats-sync-noop"
            >
              {syncReport.reason ?? "Price table already up to date."}
            </div>
          )}
          {syncReport && !syncReport.noop && (
            <div
              className="mb-3 rounded-md border border-st-await/40 bg-st-await/10 px-3 py-2 text-fg-2"
              style={{ fontSize: "10.5px" }}
              data-testid="stats-sync-report"
            >
              <div>
                {syncReport.rows} price row(s) from{" "}
                <span className="font-mono">{syncReport.source}</span>
                {syncReport.fetched_at ? ` at ${syncReport.fetched_at}` : ""}.
              </div>
              <ul className="list-disc pl-4">
                {syncReport.added.length > 0 && (
                  <li>Newly priced: {syncReport.added.join(", ")}</li>
                )}
                {syncReport.updated.length > 0 && (
                  <li>Price changed: {syncReport.updated.join(", ")}</li>
                )}
                {/* Said, never hidden: a sync cannot silently erase a hand edit. */}
                {syncReport.shadowed_by_manual.length > 0 && (
                  <li>
                    Kept from your <span className="font-mono">models.yaml</span>{" "}
                    (overrides the fetched price):{" "}
                    {syncReport.shadowed_by_manual.join(", ")}
                  </li>
                )}
                {syncReport.rejected.length > 0 && (
                  <li>Refused by the source: {syncReport.rejected.join("; ")}</li>
                )}
              </ul>
            </div>
          )}
          <Suspense
            fallback={
              <div
                className="px-1 py-8 text-center text-fg-4"
                style={{ fontSize: "11.5px" }}
                data-testid="stats-charts-loading"
              >
                Loading charts…
              </div>
            }
          >
            <StatsCharts tab={tab} overview={overview} cost={cost} costError={costError} />
          </Suspense>
        </div>
      </div>
    </div>
  );
}
