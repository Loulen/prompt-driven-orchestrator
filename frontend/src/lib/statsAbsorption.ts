import type {
  StatsAbsorbedMember,
  StatsAbsorption,
  StatsPipelineRowMeta,
  StatsProvenance,
} from "../types";

// The pure half of the Stats absorption pattern (#890, #892, ADR-0077): what a
// selectable row is, how a tab counts it, and which row the Combine modal
// proposes as the absorbent.

/** What a row absorbs: Pipelines, Nodes of one Pipeline row, or Models. */
export type AbsorptionDimension = StatsAbsorption["dimension"];

/** The word the pattern uses for a row of each dimension — never a key. */
export const NOUN: Record<AbsorptionDimension, string> = {
  pipeline: "pipeline",
  node: "node",
  model: "model",
};

/** A row the pattern can select: a Stats Pipeline, Node or Model row. */
export type AbsorbableRow = { id: string; name: string } & StatsPipelineRowMeta;

/** Where a model member's id was read from (ADR-0065 §1), said in words. */
export function provenanceNote(provenance: StatsProvenance | null | undefined): string | null {
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

export function activity(count: number, word: string, lastRun: string | null | undefined): string {
  const last = day(lastRun);
  return `${plural(count, word)} · ${last ? `last run ${last}` : "no run in this period"}`;
}

/**
 * The absorbent the modal proposes: a row that already absorbs others (it is
 * extended rather than dissolved), else the row that ran most recently — the
 * « old → new version » case validated in one gesture — then the one with the
 * most executions.
 */
export function defaultAbsorbent<T extends AbsorbableRow>(
  rows: T[],
  countOf: (row: T) => number,
): T | null {
  const rank = (row: T) => [(row.absorbed?.length ?? 0) > 0 ? 1 : 0, row.last_run ?? "", countOf(row)] as const;
  let best: T | null = null;
  for (const row of rows) {
    if (!best) {
      best = row;
      continue;
    }
    const [a0, a1, a2] = rank(row);
    const [b0, b1, b2] = rank(best);
    if (a0 !== b0 ? a0 > b0 : a1 !== b1 ? a1 > b1 : a2 > b2) best = row;
  }
  return best;
}

