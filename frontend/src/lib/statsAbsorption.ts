import type {
  StatsAbsorbedMember,
  StatsAbsorption,
  StatsPipelineRowMeta,
  StatsProvenance,
} from "../types";

// The pure half of the Stats absorption pattern (#890, #892, #906, ADR-0077,
// ADR-0078): what a selectable row is, how a tab counts it, and which row the
// Combine modal proposes as the absorbent.

/** What a row absorbs: Pipelines, Nodes of one Pipeline row, Models, the
 *  efforts of one model, or the model × effort couples of one Node row. */
export type AbsorptionDimension = StatsAbsorption["dimension"];

/** The word the pattern uses for a row of each dimension — never a key. */
export const NOUN: Record<AbsorptionDimension, string> = {
  pipeline: "pipeline",
  node: "node",
  model: "model",
  effort: "effort",
  couple: "couple",
};

/** The dimensions whose rows are ids to render mono. */
export const MONO_DIMENSIONS: ReadonlySet<AbsorptionDimension> = new Set([
  "model",
  "couple",
]);

/** The scope of a couple absorption (#906): the Node row's Pipeline and Node
 *  keys, as the daemon's `couple_scope` writes them. */
export function coupleScope(pipeline: string, node: string): string {
  return JSON.stringify([pipeline, node]);
}

/** A couple's name, as its row reads: `model · effort`. */
export function coupleName(model: string, effort: string | null): string {
  return `${model} · ${effort ?? "not set"}`;
}

/** A row the pattern can select: a Stats Pipeline, Node or Model row. */
export type AbsorbableRow = { id: string; name: string } & StatsPipelineRowMeta;

/** Where a model member's id was read from (ADR-0065 §1), said in words. */
export function provenanceNote(
  provenance: StatsProvenance | null | undefined,
): string | null {
  if (provenance === "requested") return "requested id";
  if (provenance === "mixed") return "observed and requested";
  if (provenance === "observed") return "observed id";
  return null;
}

/** How a tab counts its rows in the modals: Sessions counts executions, Cost
 *  and Performance count Runs. */
export interface AbsorptionCount<T extends AbsorbableRow> {
  of: (row: T) => number;
  ofMember: (member: StatsAbsorbedMember) => number;
  word: "execution" | "run";
}

export function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}

/** `2026-09-23T07:08:00Z` → `2026-09-23` — the day, as every Stats axis reads. */
export function day(iso: string | null | undefined): string | null {
  return iso ? iso.slice(0, 10) : null;
}

export function activity(
  count: number,
  word: string,
  lastRun: string | null | undefined,
): string {
  const last = day(lastRun);
  return `${plural(count, word)} · ${last ? `last run ${last}` : "no run in this period"}`;
}

/**
 * The absorbent the modal proposes: a `preferred` row first — on efforts and
 * couples, one with an explicit effort, so « not set » never names the series
 * when something better is selected (#906) —, then a row that already absorbs
 * others (it is extended rather than dissolved), then the row that ran most
 * recently — the « old → new version » case validated in one gesture — then the
 * one with the most executions.
 */
export function defaultAbsorbent<T extends AbsorbableRow>(
  rows: T[],
  countOf: (row: T) => number,
  preferred?: (row: T) => boolean,
): T | null {
  const rank = (row: T) =>
    [
      preferred?.(row) ? 1 : 0,
      (row.absorbed?.length ?? 0) > 0 ? 1 : 0,
      row.last_run ?? "",
      countOf(row),
    ] as const;
  let best: T | null = null;
  for (const row of rows) {
    if (!best) {
      best = row;
      continue;
    }
    const a = rank(row);
    const b = rank(best);
    const index = a.findIndex((value, i) => value !== b[i]);
    if (index >= 0 && a[index] > b[index]) best = row;
  }
  return best;
}

/** A model × effort couple of a Node as the absorption pattern selects it
 *  (#906): its key as id, `model · effort` as name. */
export type CoupleRow = AbsorbableRow & {
  model: string;
  effort: string | null;
  global_absorbed?: StatsAbsorbedMember[];
};

export function coupleRows(
  pairs: {
    key: string;
    model: string;
    effort: string | null;
    runs?: number;
    last_run?: string | null;
    absorbed?: StatsAbsorbedMember[];
    global_absorbed?: StatsAbsorbedMember[];
  }[],
): CoupleRow[] {
  return pairs.map((pair) => ({
    id: pair.key,
    name: coupleName(pair.model, pair.effort),
    model: pair.model,
    effort: pair.effort,
    runs: pair.runs,
    last_run: pair.last_run,
    absorbed: pair.absorbed,
    global_absorbed: pair.global_absorbed,
  }));
}

/** The default absorbent of efforts and couples: an explicit effort (#906). */
export const hasExplicitEffort = (row: { effort?: string | null }) =>
  row.effort !== null && row.effort !== undefined;
