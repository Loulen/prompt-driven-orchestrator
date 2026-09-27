import type { RunStatus } from "../../../shared/generated/RunStatus";

export const STATUS_STYLES: Record<RunStatus, { dot: string; ring: string }> = {
  running: { dot: "bg-st-running", ring: "border-st-running" },
  awaiting_user: { dot: "bg-st-await", ring: "border-st-await" },
  completed: { dot: "bg-st-done", ring: "border-st-done" },
  failed: { dot: "bg-st-failed", ring: "border-st-failed" },
  skipped: { dot: "bg-st-skipped", ring: "border-st-skipped" },
  halted: { dot: "bg-st-blocked", ring: "border-st-blocked" },
  paused: { dot: "bg-st-paused", ring: "border-st-paused" },
  archived: { dot: "bg-st-archived", ring: "border-st-archived" },
};

/** Runs whose bulk-Retry is valid (mirror of the per-row `canRetryAll`). */
export const RETRYABLE: readonly RunStatus[] = ["completed", "failed", "halted", "skipped"];

/**
 * Title of the bulk cleanup confirm (#815): the selected runs plus the finished
 * child runs the daemon's cascade archives with them.
 */
export function cleanupTitle(selected: number, finishedChildren: number): string {
  const runs = `${selected} run${selected === 1 ? "" : "s"}`;
  if (finishedChildren === 0) return `Cleanup ${runs}?`;
  return `Cleanup ${runs} and ${finishedChildren} finished child run${finishedChildren === 1 ? "" : "s"}?`;
}
