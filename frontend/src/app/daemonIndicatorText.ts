/** The words of the daemon's indicator of the status bar (`DaemonIndicator.tsx`). */

/** « 0.6 s » under a second, « 4 s » past it. */
export function formatElapsed(ms: number): string {
  if (ms < 1000) return `${(Math.max(ms, 0) / 1000).toFixed(1)} s`;
  return `${Math.round(ms / 1000)} s`;
}

/** The count in the ring: up to 9, then « 9+ ». */
export function ringCount(n: number): string {
  return n > 9 ? "9+" : String(n);
}
