import { Check, Combine, X } from "lucide-react";
import { IconButton } from "../../../../shared/ui/kit";

// --- The filter bar (#819) -----------------------------------------------------

/** Every tab's filters live at the top of its own pane, where their effect is
 *  read — the header keeps the period alone. One 32 px bar
 *  (`overlays.statsFilters`): the drill path on the left as a breadcrumb, the
 *  controls on the right in the order the numbers depend on them — which Runs,
 *  which duration, which Nodes, how to draw them — then the reset. */
export function StatsFilterBand({
  label,
  testid,
  children,
  onReset,
  crumbs,
}: {
  label: string;
  testid: string;
  children: React.ReactNode;
  /** `undefined` until the band deviates from the tab's defaults: a reset that
   *  resets nothing is noise. */
  onReset?: () => void;
  /** The drill path of the tab (`Breadcrumb`), on the left. */
  crumbs?: React.ReactNode;
}) {
  return (
    <div className="sx-filt" role="toolbar" aria-label={label} data-testid={testid}>
      {crumbs}
      <span className="sx-grow" />
      {children}
      {onReset && (
        <IconButton icon={X} label="Reset filters" size="sm" data-testid="stats-reset-filters" onClick={onReset} />
      )}
    </div>
  );
}

/** « Runs terminés seulement », the cohort of ONE tab (#819). Ticking it on
 *  Performance leaves Cost and Overview exactly where they were. */
export function CohortChip({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      data-testid="stats-completed-only"
      title="Count only the runs that completed"
      onClick={() => onChange(!checked)}
      className="sx-toggle"
    >
      {checked && <Check size={11} strokeWidth={2} aria-hidden="true" />}
      completed runs only
    </button>
  );
}

/** « Uncombined » (#891, ADR-0077): read the tab without the absorptions, to
 *  compare before and after without undoing anything. One reading for every
 *  tab that shows Pipelines, off at each open (« Réglages de Stats éphémères »),
 *  and shown only once the instance has an absorption to set aside. */
export function UncombinedChip({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      data-testid="stats-uncombined"
      title="Show the original rows, as if nothing were combined"
      onClick={() => onChange(!checked)}
      className="sx-toggle"
    >
      {checked ? (
        <Check size={11} strokeWidth={2} aria-hidden="true" />
      ) : (
        <Combine size={11} strokeWidth={1.5} aria-hidden="true" />
      )}
      uncombined
    </button>
  );
}

/** A thin divider between two groups of the bar, so « which Runs » and « which
 *  duration » never read as one control. */
export function BandDivider() {
  return <span className="sx-vsep" aria-hidden="true" />;
}

/** The bar of a section that filters on the cohort alone (#819): Overview,
 *  Sessions, Triggers — which share one cohort, reading one response — and
 *  Cost, which owns its own. One control whose default is « off », so unticking
 *  IS the reset and no reset button is needed. */
export function CohortOnlyBand({
  label,
  completedOnly,
  onCompletedOnlyChange,
  uncombined,
  crumbs,
  children,
}: {
  label: string;
  completedOnly: boolean;
  onCompletedOnlyChange: (value: boolean) => void;
  /** The « Uncombined » chip, on the tabs that show Pipelines (not Overview). */
  uncombined?: { checked: boolean; onChange: (value: boolean) => void };
  crumbs?: React.ReactNode;
  /** The tab's own controls (Cost's grouping), before the cohort. */
  children?: React.ReactNode;
}) {
  return (
    <StatsFilterBand label={label} testid="stats-filter-band" crumbs={crumbs}>
      {children}
      <CohortChip checked={completedOnly} onChange={onCompletedOnlyChange} />
      {uncombined && (
        <UncombinedChip
          checked={uncombined.checked}
          onChange={uncombined.onChange}
        />
      )}
    </StatsFilterBand>
  );
}
