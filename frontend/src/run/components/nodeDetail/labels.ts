import type { NodeStatus } from "../../../shared/generated/NodeStatus";

export const STATUS_LABELS: Record<NodeStatus, string> = {
  pending: "Pending",
  waiting: "Waiting",
  running: "Running",
  awaiting_user: "Awaiting you",
  completed: "Completed",
  skipped: "Skipped",
  failed: "Failed",
  stopped: "Stopped",
  stale: "Stale",
  interrupted: "Interrupted",
};
