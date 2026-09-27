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
  pending: "bg-st-pending",
  waiting: "bg-st-pending",
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  skipped: "bg-st-skipped",
  failed: "bg-st-failed",
  stopped: "bg-st-stopped",
  stale: "bg-st-stale",
  interrupted: "bg-st-interrupted",
};

export const RETRY_BUTTON_CLASS =
  "flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 transition-colors hover:bg-bg-4";
export const RETRY_BUTTON_STYLE = { fontSize: "10.5px", fontWeight: 500 } as const;
