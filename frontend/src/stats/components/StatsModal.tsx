import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { Blocks, Coins, Cpu, Gauge, Play, Receipt, RotateCw, Timer, type LucideIcon } from "lucide-react";
import FullWindowShell from "../../shared/ui/FullWindowShell";
import { Button, IconButton } from "../../shared/ui/kit";
import { StatsIcon } from "../../shared/ui/brand/AccentIcons";
import { useLocationContext, useStatsTabs } from "../../shared/extensions/catalogue";
import type { StatsTabContext } from "../../shared/extensions/contract";
import ExtensionStatsTab from "../../shared/extensions/ExtensionStatsTab";
import { fetchStatsAbsorptions } from "../api/absorptions";
import { syncCostPrices } from "../api/prices";
import { useStats } from "../store/useStats";
import type { SyncCostPricesReport } from "../model/prices";
import { periodDays, presetPeriod, PRESETS, type Preset } from "../model/period";
import type { StatsTab } from "./StatsCharts";
import { PricingDetailsContent } from "./PricingDetails";
import {
  DEFAULT_COST_VIEW,
  DEFAULT_PERFORMANCE_BAND,
  rollsUp,
  TAB_COHORT_DEFAULTS,
  type CostView,
  type PerformanceBand,
} from "../model/statsFilters";
import { useRefusal } from "../../shared/authorization/useRefusals";

/** The Operation behind « Sync costs » (`POST /stats/prices/sync`). */
const SYNC_PRICES = { resource: "price_table", verb: { action: "sync" } } as const;

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

const TABS: { id: StatsTab; label: string; icon: LucideIcon }[] = [
  { id: "runs", label: "Runs", icon: Play },
  { id: "sessions", label: "Sessions", icon: Cpu },
  { id: "triggers", label: "Triggers", icon: Timer },
  { id: "cost", label: "Cost", icon: Coins },
  { id: "performance", label: "Performance", icon: Gauge },
];

const isCoreTab = (id: string): id is StatsTab => TABS.some((item) => item.id === id);

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
  // The rail entry on screen: a core tab, or the key (`<extension>:<id>`) of a
  // tab the `stats/tab` location adds after Performance.
  const [active, setActive] = useState<string>(initialTab);
  const extensionTabs = useStatsTabs();
  const extensionTab = extensionTabs.find((item) => item.key === active) ?? null;
  // A core tab, else none: an extension's tab, or one whose extension left
  // (back to Runs then).
  const tab: StatsTab | null = isCoreTab(active) ? active : extensionTab ? null : "runs";
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
  const [costView, setCostView] = useState<CostView>(DEFAULT_COST_VIEW);
  const [pricingOpen, setPricingOpen] = useState(
    initialPricingOpen && initialTab === "cost",
  );
  const [reloadKey, setReloadKey] = useState(0);
  // Bumped by every Combine / Uncombine (#890): all tabs read the new absorptions.
  const [absorptionsVersion, setAbsorptionsVersion] = useState(0);
  // « Uncombined » (#891): off at each open like every Stats setting, one
  // reading for Sessions, Triggers, Cost and Performance. Its chip only shows
  // once the instance has an absorption to set aside.
  const [uncombined, setUncombined] = useState(false);
  const [hasAbsorptions, setHasAbsorptions] = useState(false);
  useEffect(() => {
    let cancelled = false;
    fetchStatsAbsorptions()
      .then((list) => {
        if (!cancelled) setHasAbsorptions(list.absorptions.length > 0);
      })
      .catch(() => {
        // No list, no chip: the tabs still read with the absorptions applied.
      });
    return () => {
      cancelled = true;
    };
  }, [absorptionsVersion]);
  const [syncReport, setSyncReport] = useState<SyncCostPricesReport | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncError, setSyncError] = useState<string | null>(null);
  const period = useMemo(() => presetPeriod(preset), [preset]);
  const extensionExtra = useMemo(
    () => ({ period: { preset, ...period }, refresh: reloadKey }),
    [preset, period, reloadKey],
  );
  const extensionContext = useLocationContext<StatsTabContext>("stats/tab", extensionExtra);

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
    uncombined,
    rollsUp(costView),
  );

  const refreshing =
    overviewReloadKey !== reloadKey ||
    (tab === "cost" && costReloadKey !== reloadKey) ||
    (tab === "performance" && performanceReloadKey !== reloadKey);

  // The failure of the tab on screen: its page says so, the header stops
  // announcing a computation that will not come.
  const tabError =
    tab === "cost" ? costError : tab === "performance" ? performanceError : tab ? error : null;

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
  // deliberate reading is not an anomaly to flag. The extensions' tabs follow
  // the core's.
  const rail = [
    ...TABS.map(({ id, label, icon: Icon }) => ({
      id,
      label,
      icon: <Icon size={13} strokeWidth={1.5} aria-hidden />,
    })),
    ...extensionTabs.map((item) => ({
      id: item.key,
      label: item.contribution.label,
      // A tab contribution carries no icon of its own.
      icon: <Blocks size={13} strokeWidth={1.5} aria-hidden />,
    })),
  ];
  const railLabel = rail.find((item) => item.id === (tab ?? active))?.label;
  const days = periodDays(preset, period);

  // CORE-205: Sync costs greyed, with why, when the caller may not sync the price table.
  const syncRefused = useRefusal(SYNC_PRICES, pricingOpen);
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
  // pricing panel, then Stats.
  const onEscape = () => {
    if (pricingOpen) setPricingOpen(false);
    else onClose();
  };

  return (
    <FullWindowShell
      title="Stats"
      titleIcon={<StatsIcon size={16} strokeWidth={1.5} aria-hidden />}
      subtitle={railLabel}
      testId="stats-modal"
      onClose={onClose}
      onEscape={onEscape}
      closeLabel="Close stats"
      rail={rail}
      railPlacement="top"
      railAside={
        days && (
          <span className="font-mono text-xs whitespace-nowrap text-text-2" data-testid="stats-period-days">
            {days}
          </span>
        )
      }
      activeRail={tab ?? active}
      onRailChange={(id) => {
        setActive(id);
        if (id !== "cost") setPricingOpen(false);
      }}
      railAriaLabel="Stats sections"
      railTestIdPrefix="stats-tab"
      // `scrollbar-gutter:stable` (#890): a Combine that removes rows must not
      // widen the pane — one scrollbar less made the charts reflow and print
      // x-axis labels they had been hiding. `caret-color:transparent`: nothing
      // in Stats is editable, so Chrome's caret browsing paints no blinking
      // caret on a clicked label; inputs and textareas keep theirs.
      mainClassName={`sx-main min-w-0 flex-1 overflow-y-auto bg-surface-0 [scrollbar-gutter:stable] [caret-color:transparent] [&_input]:[caret-color:auto] [&_textarea]:[caret-color:auto] ${refreshing ? "opacity-65" : ""}`}
      headerActions={
        <>
          <span
            className="text-xs whitespace-nowrap text-text-3"
            data-testid="stats-computed-at"
          >
            {computedAt
              ? `Computed ${computedAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
              : tabError
                ? null
                : "Computing…"}
          </span>
          {/* #819 — the period is the only global setting left in the bar: every
              other filter belongs to the tab it changes, in that tab's band. */}
          <div className="d-seg d-seg--sm sx-period" role="group" aria-label="Period">
            {PRESETS.map((item) => (
              <button
                key={item.id}
                type="button"
                aria-pressed={preset === item.id}
                data-testid={`stats-period-${item.id}`}
                title={item.title}
                onClick={() => setPreset(item.id)}
                className="d-seg__btn"
              >
                {item.label}
              </button>
            ))}
          </div>
          <IconButton
            icon={RotateCw}
            label="Refresh"
            onClick={refresh}
            disabled={refreshing}
            data-testid="stats-refresh"
            className={refreshing ? "sx-spin" : undefined}
          />
          {tab === "cost" && (
            // A toggle (#944): reclicking it, the panel open, closes it.
            <IconButton
              icon={Receipt}
              label="Pricing details"
              pressed={pricingOpen}
              aria-expanded={pricingOpen}
              badge={cost ? cost.total.unpriced_models.length + cost.total.missing_reasons.length || null : null}
              onClick={() => setPricingOpen((value) => !value)}
              data-testid="stats-pricing-trigger"
            />
          )}
        </>
      }
      drawer={
        pricingOpen
          ? {
              title: "Pricing details",
              escHint: "Esc returns to Stats",
              widthClassName: "w-[min(420px,calc(90vw/var(--app-zoom,1)))]",
              testId: "stats-pricing-details",
              bodyClassName: "min-h-0 flex-1 overflow-y-auto p-4",
              onClose: () => setPricingOpen(false),
              actions: (
                <Button
                  size="sm"
                  onClick={onSyncPrices}
                  disabled={syncing && !syncRefused}
                  disabledReason={syncRefused}
                  data-testid="stats-sync-prices"
                >
                  {syncing ? "Syncing…" : "Sync costs"}
                </Button>
              ),
              content: (
                <PricingDetailsContent cost={cost} syncError={syncError} syncReport={syncReport} />
              ),
            }
          : null
      }
    >
      {extensionTab && (
        // Keyed per tab: one tab's error boundary must not carry over to the next.
        <div className="sx-page">
          <ExtensionStatsTab key={extensionTab.key} tab={extensionTab} context={extensionContext} />
        </div>
      )}
      {tab && (
      <Suspense
        fallback={
          <div className="sx-empty" data-testid="stats-charts-loading">
            Loading charts…
          </div>
        }
      >
        <StatsCharts
          tab={tab}
          overview={overview}
          overviewError={error}
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
          uncombined={uncombined}
          onUncombinedChange={setUncombined}
          // Kept while on, so the reading can always be turned back off.
          showUncombined={hasAbsorptions || uncombined}
          costView={costView}
          onCostViewChange={setCostView}
        />
      </Suspense>
      )}
    </FullWindowShell>
  );
}
