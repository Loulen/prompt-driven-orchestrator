import { useEffect, useRef, useState } from "react";
import { fetchStatsOverview, fetchStatsCost, fetchStatsPerformance } from "../api";
import type { StatsOverview, StatsCost, StatsPerformance } from "../types";

/**
 * State for the Stats modal (#377, ADR-0029). Two-endpoint split by cost class:
 *
 * - **overview** (cheap indexed SQL) is fetched whenever the modal is open and
 *   the period `(from, to, bucket)` changes.
 * - **cost** (heavy, memoized) is fetched lazily — only once `costActive` is
 *   true (the user opened the cost tab) — and then refetched on period change.
 *   This keeps `/stats/cost` off the modal-open path (the two-endpoint split).
 *
 * Loading is derived from data-presence by the consumer (like `useSettings`,
 * whose open-effect never sets state synchronously); this hook only writes state
 * from the async callbacks, and — unlike `useSettings` — it *surfaces* an
 * `error`/`costError` rather than swallowing it, so a failed fetch (or a failed
 * lazy chunk on the cost tab) is visible, not a blank tab.
 *
 * `reloadKey` (#427) is a **dependency only** — bumping it refetches, and it is
 * never passed to an API call (`fetchStatsPerformance` receives `reloadKey > 0`
 * as its `refresh` flag, not the key itself). Precedent: `refreshKey` in
 * `TriggerDetailPanel`. The cohorts (#819) are the opposite: each IS part of a
 * request — the Runs the daemon must fold — so each travels to its own endpoint
 * and sits in that endpoint's request key alone. Flipping one tab's cohort
 * refetches that tab, and only that tab.
 */
export function useStats(
  open: boolean,
  from: string,
  to: string,
  bucket: string,
  costActive: boolean,
  performanceActive: boolean,
  reloadKey: number = 0,
  /**
   * « Runs terminés seulement » (#819), **per tab**. Overview, Sessions and
   * Triggers read one response, so they share `overview`; Cost and Performance
   * each own theirs. Wire default `false` everywhere — the caller decides which
   * tab opens narrowed.
   */
  cohorts: { overview?: boolean; cost?: boolean; performance?: boolean } = {},
  /**
   * Bumped after every Combine / Uncombine (#890). Like `reloadKey`, a
   * **dependency only** — but it never asks Performance to bypass its memo: the
   * daemon keys that memo on the absorptions already (ADR-0077 §2). The data on
   * screen stays until the new answer lands, so a Combine never blanks a tab.
   */
  absorptionsVersion: number = 0,
) {
  const {
    overview: overviewCompletedOnly = false,
    cost: costCompletedOnly = false,
    performance: performanceCompletedOnly = false,
  } = cohorts;
  const [overview, setOverview] = useState<StatsOverview | null>(null);
  const [cost, setCost] = useState<StatsCost | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [costError, setCostError] = useState<string | null>(null);
  const [performance, setPerformance] = useState<StatsPerformance | null>(null);
  const [performanceError, setPerformanceError] = useState<string | null>(null);
  const [computedAt, setComputedAt] = useState<Date | null>(null);
  const [overviewReloadKey, setOverviewReloadKey] = useState(0);
  const [costReloadKey, setCostReloadKey] = useState(0);
  const [performanceReloadKey, setPerformanceReloadKey] = useState(0);
  const costRequestKey = `${from}\u0000${to}\u0000${bucket}\u0000${reloadKey}\u0000${costCompletedOnly}\u0000${absorptionsVersion}`;
  const requestedCostKey = useRef<string | null>(null);
  const performanceRequestKey = `${from}\u0000${to}\u0000${reloadKey}\u0000${performanceCompletedOnly}\u0000${absorptionsVersion}`;
  const requestedPerformanceKey = useRef<string | null>(null);

  // Overview: eager on open + on every period change.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchStatsOverview(from, to, bucket, overviewCompletedOnly)
      .then((data) => {
        if (!cancelled) {
          setOverview(data);
          setError(null);
          setComputedAt(new Date());
          setOverviewReloadKey(reloadKey);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
          setOverviewReloadKey(reloadKey);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, from, to, bucket, reloadKey, overviewCompletedOnly, absorptionsVersion]);

  // Cost: lazy — only once the cost tab is active, then on period change too.
  useEffect(() => {
    if (!open || !costActive) return;
    if (requestedCostKey.current === costRequestKey) return;
    requestedCostKey.current = costRequestKey;
    fetchStatsCost(from, to, bucket, costCompletedOnly)
      .then((data) => {
        if (requestedCostKey.current === costRequestKey) {
          setCost(data);
          setCostError(null);
          setComputedAt(new Date());
          setCostReloadKey(reloadKey);
        }
      })
      .catch((e) => {
        if (requestedCostKey.current === costRequestKey) {
          setCostError(e instanceof Error ? e.message : String(e));
          setCostReloadKey(reloadKey);
        }
      });
  }, [
    open,
    costActive,
    from,
    to,
    bucket,
    reloadKey,
    costCompletedOnly,
    costRequestKey,
  ]);

  useEffect(() => {
    if (!open || !performanceActive) return;
    if (requestedPerformanceKey.current === performanceRequestKey) return;
    requestedPerformanceKey.current = performanceRequestKey;
    fetchStatsPerformance(from, to, reloadKey > 0, performanceCompletedOnly)
      .then((data) => {
        if (requestedPerformanceKey.current === performanceRequestKey) {
          setPerformance(data);
          setPerformanceError(null);
          setComputedAt(new Date());
          setPerformanceReloadKey(reloadKey);
        }
      })
      .catch((e) => {
        if (requestedPerformanceKey.current === performanceRequestKey) {
          setPerformanceError(e instanceof Error ? e.message : String(e));
          setPerformanceReloadKey(reloadKey);
        }
      });
  }, [
    open,
    performanceActive,
    from,
    to,
    reloadKey,
    performanceCompletedOnly,
    performanceRequestKey,
  ]);

  return {
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
  };
}
