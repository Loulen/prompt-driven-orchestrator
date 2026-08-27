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

/** Cost text shared by aggregate cells for derived and harness-reported values. */
export function formatCostAmount(
  usd: number | null,
  partial: boolean,
  estimated: boolean,
): string {
  if (usd === null) return "—";
  const amount = `$${usd.toFixed(costPrecision(usd))}`;
  return `${estimated ? `~${amount}` : amount}${partial ? "†" : ""}`;
}

/** Base note framing any cost figure as an estimate (matches `/estimate/i`). */
export const COST_ESTIMATE_NOTE =
  "Estimate from local Claude Code token usage × public list prices — not an invoice.";

/** Generic lower-bound clause, used only when the excluded model's name is not
 *  available (matches `/lower bound/i`). The named form (#425) is preferred. */
export const COST_LOWER_BOUND_NOTE = " Lower bound: an unpriced model was excluded.";

/**
 * The lower-bound clause for a tooltip. Names the excluded model family keys
 * when known (#425 AC#4 — "an unpriced model" was invisible enough to hide the
 * priciest model for weeks), else falls back to the generic note. `runSuffix`
 * (e.g. `" (2 partial runs)."`) is appended by the aggregate bucket and omitted
 * for a single run.
 */
function lowerBoundClause(unpricedModels: string[], runSuffix = ""): string {
  const body =
    unpricedModels.length > 0
      ? ` Lower bound: unpriced ${
          unpricedModels.length === 1 ? "model" : "models"
        } excluded: ${unpricedModels.join(", ")}.`
      : COST_LOWER_BOUND_NOTE;
  return body + runSuffix;
}

/**
 * The clause for a Run whose cost is **unavailable** because one or more harnesses
 * has no cost source (#553, ADR-0045). Names the harness(es) — the same "name what
 * is missing" discipline as {@link lowerBoundClause} for unpriced models — so the
 * user never reads an anonymous blank, and never a `$0` standing in for "unknown".
 * A categorically different state from a lower bound: there is no figure at all.
 */
export function uncostedClause(uncostedHarnesses: string[]): string {
  return ` Cost unavailable: ${
    uncostedHarnesses.length === 1 ? "harness" : "harnesses"
  } ${uncostedHarnesses.join(", ")} ${
    uncostedHarnesses.length === 1 ? "has" : "have"
  } no cost source, so this Run's cost cannot be estimated.`;
}

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
 * model — naming which model(s) when known (#425).
 */
export function formatEstCost(
  usd: number,
  partial: boolean,
  unpricedModels: string[] = [],
  uncostedHarnesses: string[] = [],
): CostLabel {
  // #553: a harness with no cost source makes the Run's cost not honestly
  // summable — show "—" with a reason naming the harness, never a $ figure and
  // never a mute dagger (that would read as "priced, lower bound", which this is
  // not). This branch takes precedence over `partial`, since "unavailable" is a
  // stronger statement than "incomplete".
  if (uncostedHarnesses.length > 0) {
    return {
      text: "—",
      dagger: false,
      title: COST_ESTIMATE_NOTE + uncostedClause(uncostedHarnesses),
    };
  }
  return {
    text: `~$${usd.toFixed(costPrecision(usd))}`,
    dagger: partial,
    title: COST_ESTIMATE_NOTE + (partial ? lowerBoundClause(unpricedModels) : ""),
  };
}
