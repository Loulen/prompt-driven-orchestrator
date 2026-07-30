import { useEffect, useState } from "react";
import { fetchStatsOverview, fetchStatsCost } from "../api";
import type { StatsOverview, StatsCost } from "../types";

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
 * never passed to an API call. Threading it into `fetchStatsCost` would change
 * that function's arity, which the tests below assert exactly (Vitest compares
 * arity strictly). Precedent: `refreshKey` in `TriggerDetailPanel`.
 */
export function useStats(
  open: boolean,
  from: string,
  to: string,
  bucket: string,
  costActive: boolean,
  reloadKey: number = 0,
) {
  const [overview, setOverview] = useState<StatsOverview | null>(null);
  const [cost, setCost] = useState<StatsCost | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [costError, setCostError] = useState<string | null>(null);

  // Overview: eager on open + on every period change.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchStatsOverview(from, to, bucket)
      .then((data) => {
        if (!cancelled) {
          setOverview(data);
          setError(null);
        }
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, from, to, bucket, reloadKey]);

  // Cost: lazy — only once the cost tab is active, then on period change too.
  useEffect(() => {
    if (!open || !costActive) return;
    let cancelled = false;
    fetchStatsCost(from, to, bucket)
      .then((data) => {
        if (!cancelled) {
          setCost(data);
          setCostError(null);
        }
      })
      .catch((e) => {
        if (!cancelled) setCostError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, costActive, from, to, bucket, reloadKey]);

  return { overview, cost, error, costError };
}
