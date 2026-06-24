import { useState, useEffect } from "react";
import { isLiveRun } from "../types";
import type { RunStatus } from "../types";

/**
 * Format an elapsed-milliseconds duration as a compact ladder (`1h 23m` /
 * `4m 12s` / `45s`). Returns `null` for a missing/negative/non-finite input so
 * the caller can render "—". Zero-dep (no date library in the repo). (#100)
 */
export function formatDuration(ms: number | null): string | null {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return null;
  const totalSecs = Math.floor(ms / 1000);
  const secs = totalSecs % 60;
  const mins = Math.floor(totalSecs / 60) % 60;
  const hrs = Math.floor(totalSecs / 3600);
  if (hrs > 0) return `${hrs}h ${mins}m`;
  if (mins > 0) return `${mins}m ${secs}s`;
  return `${secs}s`;
}

/**
 * Wall-clock duration of a run in milliseconds, derived client-side (#100).
 * Ticks once a second while the run is live and not yet completed; freezes at
 * `completed_at` once terminal. Returns `null` when `startedAt` is missing or
 * unparseable. Wall-clock includes paused spans (J3). Modeled on
 * `useRelativeTime` in TabBar.tsx.
 */
export function useRunDuration(
  startedAt: string | null | undefined,
  completedAt: string | null | undefined,
  status: RunStatus | undefined,
): number | null {
  const [now, setNow] = useState(() => Date.now());
  const ticking = !!startedAt && completedAt == null && status != null && isLiveRun(status);

  useEffect(() => {
    if (!ticking) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [ticking]);

  if (!startedAt) return null;
  const start = Date.parse(startedAt);
  if (Number.isNaN(start)) return null;
  const end = completedAt ? Date.parse(completedAt) : now;
  if (Number.isNaN(end)) return null;
  return Math.max(0, end - start);
}
