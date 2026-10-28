import { Check, Combine } from "lucide-react";

// --- The filter band (#819) ----------------------------------------------------

/** Every tab's filters live at the top of its own pane, where their effect is
 *  read — the shell bar keeps the period alone. One frame, one legend naming
 *  the tab, controls left → right in the order the numbers depend on them:
 *  which Runs, which duration, which Nodes, how to draw them. */
export function StatsFilterBand({
  label,
  testid,
  children,
  onReset,
}: {
  label: string;
  testid: string;
  children: React.ReactNode;
  /** `undefined` until the band deviates from the tab's defaults: a reset that
   *  resets nothing is noise. */
  onReset?: () => void;
}) {
  return (
    <fieldset
      className="mb-3 rounded-md border border-line px-3 pb-2 pt-1"
      data-testid={testid}
      style={{ fontSize: "11px" }}
    >
      <legend
        className="px-1 uppercase tracking-wide text-st-done"
        style={{ fontSize: "9px" }}
      >
        {label}
      </legend>
      <div className="flex flex-wrap items-center gap-2">
        {children}
        {onReset && (
          <button
            type="button"
            data-testid="stats-reset-filters"
            onClick={onReset}
            className="ml-auto text-text-3 underline decoration-dotted underline-offset-2 hover:text-text-2"
          >
            reset filters
          </button>
        )}
      </div>
    </fieldset>
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
      onClick={() => onChange(!checked)}
      className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 ${
        checked
          ? "border-st-done bg-st-done/15 text-text"
          : "border-line-strong bg-surface-2 text-text-2"
      }`}
    >
      {checked && <Check size={10} strokeWidth={3} aria-hidden="true" />}
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
      className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 ${
        checked
          ? "border-st-done bg-st-done/15 text-text"
          : "border-line-strong bg-surface-2 text-text-2"
      }`}
    >
      {checked ? (
        <Check size={10} strokeWidth={3} aria-hidden="true" />
      ) : (
        <Combine size={10} aria-hidden="true" />
      )}
      uncombined
    </button>
  );
}

/** A thin divider between two groups of the band, so « which Runs » and « which
 *  duration » never read as one control. */
export function BandDivider() {
  return <span className="mx-1 h-4 w-px bg-line-strong" aria-hidden="true" />;
}

/** The band of a section that filters on the cohort alone (#819): Overview,
 *  Sessions, Triggers — which share one cohort, reading one response — and
 *  Cost, which owns its own. One control whose default is « off », so unticking
 *  IS the reset and no reset link is needed. */
export function CohortOnlyBand({
  label,
  completedOnly,
  onCompletedOnlyChange,
  uncombined,
}: {
  label: string;
  completedOnly: boolean;
  onCompletedOnlyChange: (value: boolean) => void;
  /** The « Uncombined » chip, on the tabs that show Pipelines (not Overview). */
  uncombined?: { checked: boolean; onChange: (value: boolean) => void };
}) {
  return (
    <StatsFilterBand label={label} testid="stats-filter-band">
      <CohortChip checked={completedOnly} onChange={onCompletedOnlyChange} />
      <span className="text-text-3">default: all runs</span>
      {uncombined && (
        <UncombinedChip
          checked={uncombined.checked}
          onChange={uncombined.onChange}
        />
      )}
    </StatsFilterBand>
  );
}
