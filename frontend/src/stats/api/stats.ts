/**
 * The Stats tabs' figures: the overview, the cost and the performance over a
 * period.
 */

import type { StatsOverview } from "../../shared/generated/StatsOverview";
import type { StatsCost } from "../../shared/generated/StatsCost";
import type { StatsPerformance } from "../../shared/generated/StatsPerformance";
import { request } from "../../shared/http/client";

/**
 * Cheap instance stats over `[from, to)` bucketed by `bucket` (#377): runs,
 * errors (`run_failed`), sessions, fires-per-pipeline, and the "triggers that
 * created a run" KPI. Indexed SQL — safe to fetch on modal open.
 * `completed_only` (#810) narrows the Run cohort to Runs that reached
 * `completed`; the Errors series then reads zero rather than disappearing.
 */
export function fetchStatsOverview(
  from: string,
  to: string,
  bucket: string,
  completedOnly = false,
  uncombined = false,
): Promise<StatsOverview> {
  return request<StatsOverview>("GET", "/stats/overview", {
    query: {
      from,
      to,
      bucket,
      ...(completedOnly ? { completed_only: true } : {}),
      // #891 — « Uncombined »: the rows as the event log wrote them. Omitted
      // when off, so an unchanged call sends the query it always did.
      ...(uncombined ? { uncombined: true } : {}),
    },
  });
}

/**
 * Estimated cost over `[from, to)`, folded by period/pipeline/project (#377,
 * ADR-0022/0029). Heavy (memoized per-run cost fanned over the window) — fetch
 * lazily, only when the cost tab is shown. `rollUp` — « Roll up child runs »:
 * the cohort becomes the root runs of the period, each child run filed under
 * its root's Pipeline and Project; omitted when off.
 */
export function fetchStatsCost(
  from: string,
  to: string,
  bucket: string,
  completedOnly = false,
  uncombined = false,
  rollUp = false,
): Promise<StatsCost> {
  return request<StatsCost>("GET", "/stats/cost", {
    query: {
      from,
      to,
      bucket,
      ...(completedOnly ? { completed_only: true } : {}),
      ...(uncombined ? { uncombined: true } : {}),
      ...(rollUp ? { roll_up_children: true } : {}),
    },
  });
}

/**
 * Context, wall-clock and **active** duration distributions. Heavy journal
 * reads, so callers load it lazily. `completed_only` (#810) narrows the cohort
 * to Runs that reached `completed`; omitted when false, so an unchanged call
 * sends the byte-identical query it always did.
 */
export function fetchStatsPerformance(
  from: string,
  to: string,
  refresh = false,
  completedOnly = false,
  uncombined = false,
): Promise<StatsPerformance> {
  return request<StatsPerformance>("GET", "/stats/performance", {
    query: {
      from,
      to,
      ...(refresh ? { refresh: true } : {}),
      ...(completedOnly ? { completed_only: true } : {}),
      ...(uncombined ? { uncombined: true } : {}),
    },
  });
}
