import type { NodeStatus } from "./shared/generated/NodeStatus";

export const STATUS_BORDER: Record<NodeStatus, string> = {
  pending: "border-line-strong",
  // Admission-throttled (session cap): not started yet, drawn like pending.
  waiting: "border-line-strong",
  running: "border-st-running",
  awaiting_user: "border-st-await",
  completed: "border-st-done",
  skipped: "border-st-skipped",
  failed: "border-st-failed",
  stopped: "border-st-skipped",
  stale: "border-st-skipped",
  interrupted: "border-st-await",
};

export const STATUS_BG: Record<NodeStatus, string> = {
  pending: "bg-surface-2",
  waiting: "bg-surface-2",
  running: "bg-st-running/14",
  awaiting_user: "bg-st-await/14",
  completed: "bg-st-done/14",
  skipped: "bg-st-skipped/14",
  failed: "bg-st-failed/14",
  stopped: "bg-st-skipped/14",
  stale: "bg-st-skipped/14",
  interrupted: "bg-st-await/14",
};

/** Status-tinted text, for a badge that carries the status colour (#44). */
export const STATUS_TEXT: Record<NodeStatus, string> = {
  pending: "text-text-3",
  waiting: "text-text-3",
  running: "text-st-running",
  awaiting_user: "text-st-await",
  completed: "text-st-done",
  skipped: "text-st-skipped",
  failed: "text-st-failed",
  stopped: "text-st-skipped",
  stale: "text-st-skipped",
  interrupted: "text-st-await",
};

export const STATUS_DOT: Record<NodeStatus, string> = {
  pending: "bg-st-skipped",
  waiting: "bg-st-skipped",
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  skipped: "bg-st-skipped",
  failed: "bg-st-failed",
  stopped: "bg-st-skipped",
  stale: "bg-st-skipped",
  interrupted: "bg-st-await",
};

export const SELECTION_RING_STYLE = {
  boxShadow: "0 0 0 2px var(--surface-0), 0 0 0 3.5px var(--primary)",
};
