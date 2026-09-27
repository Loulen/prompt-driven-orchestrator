/**
 * The Performance tab's pure rules: its metrics and the wire field each reads
 * under the duration mode (#819), the box-plot zoom levels (#811), the ranking
 * score and the value formats.
 */

import type { StatsDistribution } from "../../shared/generated/StatsDistribution";
import type { StatsPerformanceAggregate } from "../../shared/generated/StatsPerformanceAggregate";
import type { StatsSteeredRate } from "../../shared/generated/StatsSteeredRate";
import type { DurationMode, StatsZoom } from "./statsFilters";

export type PerformanceMetric = "context" | "duration" | "steering";

export const PERFORMANCE_METRICS: readonly PerformanceMetric[] = [
  "context",
  "duration",
  "steering",
];

/**
 * The box-plot zoom level (#811, CONTEXT.md « Niveau de zoom d'un box-plot »):
 * one setting for the whole Performance section, deciding what a plot draws AND
 * where the axis tops out. The three levels differ only by which pair of
 * statistics bounds the drawing — everything below reads these two tables
 * rather than branching on the level by hand.
 *
 * The daemon never sends the observations (ADR-0029 / the glossary), so a level
 * can only be a choice among the statistics it already sent: that is why
 * `fence_low`/`fence_high` are computed server-side.
 */
export const ZOOM_LOW: Record<StatsZoom, "min" | "fence_low" | "q1"> = {
  full: "min",
  fenced: "fence_low",
  box: "q1",
};

export const ZOOM_HIGH: Record<StatsZoom, "max" | "fence_high" | "q3"> = {
  full: "max",
  fenced: "fence_high",
  box: "q3",
};

export const ZOOM_LEVELS: readonly StatsZoom[] = ["full", "fenced", "box"];

export const ZOOM_LABEL: Record<StatsZoom, string> = {
  full: "Full",
  fenced: "Fenced",
  box: "Box",
};

/** What the shared axis is capped at, said in the toolbar's caption. */
export const ZOOM_CAP_COPY: Record<StatsZoom, string> = {
  full: "max",
  fenced: "fence high",
  box: "Q3",
};

/** The wire field a metric actually reads. The **duration mode** (#819) picks
 *  which of Duration's three distributions is read — wall-clock, minus the
 *  declared wait, or that wait alone — with **no refetch**: the three always
 *  travel together, over the same executions and the same coverage. */
export type PerformanceField =
  "context" | "duration" | "active_duration" | "wait_duration" | "steering";

/** Which duration field each mode reads. */
export const MODE_FIELD: Record<DurationMode, PerformanceField> = {
  total: "duration",
  active: "active_duration",
  waiting: "wait_duration",
};

export function metricField(
  metric: PerformanceMetric,
  mode: DurationMode,
): PerformanceField {
  return metric === "duration" ? MODE_FIELD[mode] : metric;
}

/** The logical metric a field belongs to — what decides its unit and format.
 *  All three duration readings are durations: same milliseconds, same `6m20s`. */
export function fieldMetric(field: PerformanceField): PerformanceMetric {
  return field === "active_duration" || field === "wait_duration"
    ? "duration"
    : field;
}

export const PERFORMANCE_FIELD_LABEL: Record<PerformanceField, string> = {
  context: "Context",
  duration: "Duration",
  active_duration: "Active",
  wait_duration: "Waiting",
  steering: "Steering",
};

/** What a **declared wait** is, said in words (#819): the rule, not a decision
 *  reference. The mode's « i », the column header and the cards share it. */
export const DECLARED_WAIT_COPY =
  "a declared wait is a node waiting for you (pdo wait-user) or a completion you have not released yet";

/** Why a partially filtered row cannot show a number (#810): six stats are not
 *  observations, so no honest total can be rebuilt from the visible nodes. */
export const FILTERED_BY_KIND_COPY =
  "Filtered by node kind: totals are not recomputed from the visible nodes' six stats";

/** « duration » / « active duration » / « waiting » — the word every Performance
 *  label uses under the current mode, so the headline, the cards, the sort
 *  select and the aside never disagree about what is being measured. */
export const DURATION_WORD: Record<DurationMode, string> = {
  total: "duration",
  active: "active duration",
  waiting: "waiting",
};

export function durationWord(mode: DurationMode): string {
  return DURATION_WORD[mode];
}

export function performanceSortLabel(
  metric: PerformanceMetric,
  mode: DurationMode,
): string {
  return metric === "duration" ? durationWord(mode) : metric;
}

/** Where the Steering metric comes from (#792) — the header « i », the card
 *  « i » and every steering tooltip say it in the same words. */
export const STEERING_PROVENANCE_COPY =
  "derived from harness transcripts, launch prompt and runtime messages excluded";

/** « 23 % » for a steered rate, or `null` when no execution's count was
 *  readable — the caller renders « — » and the reason, never 0 %. */
export function steeredPercent(rate: StatsSteeredRate | undefined): string | null {
  if (!rate || rate.readable === 0) return null;
  return `${Math.round((rate.steered / rate.readable) * 100)} %`;
}

/** A row's ranking score on a metric: its worst harness's **median** first, the
 *  mean only as a tie-break (#811 — « Médiane, jamais la moyenne »: the median
 *  ranks and labels, the mean survives in the tooltip). `-1` for a row with no
 *  measured distribution at all, so it sinks below any real value. */
export function performanceScore(
  aggregate: StatsPerformanceAggregate,
  metric: PerformanceMetric,
  mode: DurationMode = "total",
): [number, number] {
  const field = metricField(metric, mode);
  const distributions = aggregate.harnesses
    .map((item) => item[field])
    .filter((item) => item.stats !== null);
  return [
    Math.max(-1, ...distributions.map((item) => item.stats!.median)),
    Math.max(-1, ...distributions.map((item) => item.stats!.mean)),
  ];
}

export function sortPerformance<
  T extends StatsPerformanceAggregate & { name: string },
>(rows: T[], metric: PerformanceMetric, mode: DurationMode = "total"): T[] {
  return [...rows].sort((a, b) => {
    const [aMedian, aMean] = performanceScore(a, metric, mode);
    const [bMedian, bMean] = performanceScore(b, metric, mode);
    return bMedian - aMedian || bMean - aMean || a.name.localeCompare(b.name);
  });
}

export function formatPerformanceValue(
  value: number,
  metric: PerformanceMetric,
): string {
  if (metric === "context") {
    return value >= 1_000
      ? `${Math.round(value / 1_000)}k`
      : Math.round(value).toString();
  }
  if (metric === "steering") {
    // Messages: an integer reads as one (`0`, `2`), a mean keeps one decimal (`0.8`).
    const rounded = Math.round(value * 10) / 10;
    return Number.isInteger(rounded) ? rounded.toString() : rounded.toFixed(1);
  }
  const seconds = Math.round(value / 1_000);
  const minutes = Math.floor(seconds / 60);
  return minutes
    ? `${minutes}m${String(seconds % 60).padStart(2, "0")}s`
    : `${seconds}s`;
}

export function distributionDetail(
  name: string,
  harness: string,
  field: PerformanceField,
  value: StatsDistribution,
  /** The six numbers never move; the Fenced level appends the two bounds it
   *  draws its whiskers at, so the reader can name what cropped the plot. */
  zoom: StatsZoom = "full",
): string {
  const metric = fieldMetric(field);
  const label = PERFORMANCE_FIELD_LABEL[field];
  const fmt = (raw: number) => formatPerformanceValue(raw, metric);
  const stats = value.stats;
  const provenance =
    metric === "steering"
      ? ` Steering ${STEERING_PROVENANCE_COPY}.`
      : field === "active_duration"
        ? ` Active: wall-clock minus the declared waits — ${DECLARED_WAIT_COPY}.`
        : field === "wait_duration"
          ? ` Waiting: the declared waits alone — ${DECLARED_WAIT_COPY}.`
          : "";
  if (!stats) {
    return `${name} · ${harness} · ${label}. 0 measured of ${value.expected} successful executions. Missing: ${value.missing_reasons.join("; ")}.${provenance}`;
  }
  const reasons = value.missing_reasons.length
    ? ` Missing: ${value.missing_reasons.join("; ")}.`
    : "";
  const fences =
    zoom === "fenced"
      ? ` · Fence high ${fmt(stats.fence_high)} · Fence low ${fmt(stats.fence_low)}`
      : "";
  return `${name} · ${harness} · ${label}. Max ${fmt(stats.max)} · Q3 ${fmt(stats.q3)} · Mean ${fmt(stats.mean)} · Median ${fmt(stats.median)} · Q1 ${fmt(stats.q1)} · Min ${fmt(stats.min)}${fences}. ${value.measured} measured of ${value.expected} successful executions.${reasons}${provenance}`;
}

export type PerformanceAxis = "pipeline" | "model";
