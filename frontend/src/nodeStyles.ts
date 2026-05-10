import type { NodeStatus } from "./types";

export const STATUS_BORDER: Record<NodeStatus, string> = {
  pending: "border-line-strong",
  running: "border-st-running",
  awaiting_user: "border-st-await",
  completed: "border-st-done",
  failed: "border-st-failed",
};

export const STATUS_BG: Record<NodeStatus, string> = {
  pending: "bg-bg-3",
  running: "bg-st-running-bg",
  awaiting_user: "bg-st-await-bg",
  completed: "bg-st-done-bg",
  failed: "bg-st-failed-bg",
};

export const STATUS_DOT: Record<NodeStatus, string> = {
  pending: "bg-st-pending",
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
};

export const SELECTION_RING_STYLE = {
  boxShadow: "0 0 0 2px var(--color-bg-1), 0 0 0 3.5px var(--color-acc)",
};
