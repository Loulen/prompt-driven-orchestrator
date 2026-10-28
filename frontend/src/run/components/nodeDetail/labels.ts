import type { NodeStatus } from "../../../shared/generated/NodeStatus";

export const STATUS_LABELS: Record<NodeStatus, string> = {
  pending: "Pending",
  waiting: "Waiting",
  running: "Running",
  awaiting_user: "Awaiting User",
  completed: "Completed",
  skipped: "Skipped",
  failed: "Failed",
  stopped: "Stopped",
  stale: "Stale",
  interrupted: "Interrupted",
};

export const STATUS_DOTS: Record<NodeStatus, string> = {
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

export const RETRY_BUTTON_CLASS =
  "flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-2 transition-colors hover:bg-hover";
export const RETRY_BUTTON_STYLE = { fontSize: "10.5px", fontWeight: 500 } as const;
