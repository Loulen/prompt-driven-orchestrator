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
 * `TriggerDetailPanel`. `completedOnly` (#810) is the opposite: it IS part of
 * the request — the cohort the daemon must fold — so it travels to all three
 * endpoints and into both request keys.
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
   * « Runs terminés seulement » (#810) — the cohort, not a display option: it
   * travels to the THREE fetches so Overview, Cost and Performance always
   * describe the same Runs, and it sits in every request key so flipping it
   * refetches rather than showing the other cohort's numbers.
   */
  completedOnly: boolean = false,
) {
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
  const costRequestKey = `${from}\u0000${to}\u0000${bucket}\u0000${reloadKey}\u0000${completedOnly}`;
  const requestedCostKey = useRef<string | null>(null);
  const performanceRequestKey = `${from}\u0000${to}\u0000${reloadKey}\u0000${completedOnly}`;
  const requestedPerformanceKey = useRef<string | null>(null);

  // Overview: eager on open + on every period change.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchStatsOverview(from, to, bucket, completedOnly)
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
  }, [open, from, to, bucket, reloadKey, completedOnly]);

  // Cost: lazy — only once the cost tab is active, then on period change too.
  useEffect(() => {
    if (!open || !costActive) return;
    if (requestedCostKey.current === costRequestKey) return;
    requestedCostKey.current = costRequestKey;
    fetchStatsCost(from, to, bucket, completedOnly)
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
    completedOnly,
    costRequestKey,
  ]);

  useEffect(() => {
    if (!open || !performanceActive) return;
    if (requestedPerformanceKey.current === performanceRequestKey) return;
    requestedPerformanceKey.current = performanceRequestKey;
    fetchStatsPerformance(from, to, reloadKey > 0, completedOnly)
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
    completedOnly,
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
