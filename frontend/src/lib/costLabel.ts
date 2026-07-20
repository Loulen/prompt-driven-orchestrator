// Honest cost labelling, shared by the per-run stat (PipelineInfoPanel, #272)
// and the aggregated Stats charts (#377). ADR-0001 (sharp tool, honest labels) +
// ADR-0022 (estimate from local transcripts, never an invoice): every cost the
// UI shows is framed as an estimate; any unpriced-model contribution makes it a
// lower bound (`†`); an uncomputable/empty bucket renders `—`, never `$0`.
//
// The vocabulary lives here ONCE so the per-run row and the charts stay
// byte-identical.

/** Adaptive precision: sub-dollar estimates show 4 decimals, else 2 (#272). */
export function costPrecision(usd: number): number {
  return usd < 1 ? 4 : 2;
}

/** Base note framing any cost figure as an estimate (matches `/estimate/i`). */
export const COST_ESTIMATE_NOTE =
  "Estimate from local Claude Code token usage × public list prices — not an invoice.";

/** Lower-bound clause appended when an unpriced model was excluded (matches
 *  `/lower bound/i`). Byte-identical between the per-run row and the charts. */
export const COST_LOWER_BOUND_NOTE = " Lower bound: an unpriced model was excluded.";

export interface CostLabel {
  /** Display text, e.g. `~$1.2345`. */
  text: string;
  /** Whether to render the `†` lower-bound marker. */
  dagger: boolean;
  /** Full tooltip string. */
  title: string;
}

/**
 * Format a single run's estimated cost (#272): `~$X` at adaptive precision, with
 * a `†` marker and a "lower bound" note when the estimate excluded an unpriced
 * model.
 */
export function formatEstCost(usd: number, partial: boolean): CostLabel {
  return {
    text: `~$${usd.toFixed(costPrecision(usd))}`,
    dagger: partial,
    title: COST_ESTIMATE_NOTE + (partial ? COST_LOWER_BOUND_NOTE : ""),
  };
}

export interface CostBucketLabel extends CostLabel {
  /** Nothing priced (no runs, or every run lacked a transcript) → render `—`. */
  empty: boolean;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/**
 * Format an aggregated cost bucket (#377). A bucket is a **sum of lower bounds**:
 *
 * - any `partial` run makes the whole bucket a lower bound (`†`);
 * - runs with no transcript (`nullCount`) are excluded from `usd` but surfaced
 *   in the tooltip so the bucket is never silently undercounted;
 * - a bucket with nothing priced (`runs === 0`, or every run was null) renders
 *   `—`, never `$0` (a wrong number, not a placeholder).
 */
export function formatBucketCost(
  usd: number,
  partialCount: number,
  nullCount: number,
  runs: number,
): CostBucketLabel {
  const priced = runs - nullCount;
  const empty = priced <= 0;
  const partial = partialCount > 0;

  let title = COST_ESTIMATE_NOTE;
  if (partial) {
    title += `${COST_LOWER_BOUND_NOTE} (${plural(partialCount, "partial run")}).`;
  }
  if (nullCount > 0) {
    title += ` ${plural(nullCount, "run")} had no transcript (excluded).`;
  }

  return {
    text: empty ? "—" : `~$${usd.toFixed(costPrecision(usd))}`,
    dagger: partial,
    title,
    empty,
  };
}
