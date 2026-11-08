/**
 * The Cost tab's pure rules: what a cost cell's tooltip says, the aggregate a
 * model × effort couple reads as, and which Node-level rows may combine.
 */

import type { StatsCostEntity } from "../../shared/generated/StatsCostEntity";
import type { StatsHarnessCost } from "../../shared/generated/StatsHarnessCost";
import type { StatsModelEffortPair } from "../../shared/generated/StatsModelEffortPair";

export type CostAxis = "pipeline" | "project" | "model";

/** A Cost Node level lists the Infrastructure and Unassigned buckets beside the
 *  Nodes: those are no Node, so they never combine (#892). */
export const isCostNodeRow = (row: StatsCostEntity) =>
  !row.id.endsWith(":infrastructure") && !row.id.endsWith(":unassigned");

export function coverage(metric: StatsHarnessCost, unit: "Run" | "execution"): string {
  const parts = [
    `${metric.readable} readable ${metric.readable === 1 ? "cost" : "costs"} of ${metric.executions} ${
      metric.executions === 1 ? unit : `${unit}s`
    }`,
  ];
  if (metric.unpriced_models.length) {
    parts.push(`Lower bound; unpriced: ${metric.unpriced_models.join(", ")}`);
  }
  if (metric.missing_reasons.length)
    parts.push(metric.missing_reasons.join("; "));
  return parts.join(". ");
}

/** A cost row's own figures, read as the « total » column's cell. */
export function rowMetric(row: StatsCostEntity): StatsHarnessCost {
  return {
    harness: "total",
    usd: row.usd,
    estimated: row.estimated,
    partial: row.partial,
    executions: row.executions,
    readable: row.readable,
    unknown: row.unknown,
    average_usd: row.average_usd,
    median_usd: row.median_usd,
    unpriced_models: row.unpriced_models,
    missing_reasons: row.missing_reasons,
  };
}

export function pairMetric(pair: StatsModelEffortPair): StatsHarnessCost {
  return {
    harness: "total",
    usd: pair.usd,
    estimated: pair.estimated,
    partial: pair.partial,
    executions: pair.executions,
    readable: pair.readable,
    unknown: pair.unknown,
    average_usd: pair.average_usd,
    median_usd: pair.median_usd,
    unpriced_models: pair.unpriced_models,
    missing_reasons: pair.missing_reasons,
  };
}
