import type { NodeStatus, NodeType } from "./types";

export const TYPE_LABELS: Record<NodeType, string> = {
  "doc-only": "doc",
  "code-mutating": "code",
  "start": "start",
  "end": "end",
  "switch": "switch",
  "loop": "loop",
  "for-each": "foreach",
};

export const TYPE_COLORS: Record<NodeType, string> = {
  "doc-only": "border-st-pending text-fg-3",
  "code-mutating": "border-acc text-acc",
  "start": "border-acc text-acc",
  "end": "border-st-blocked text-st-blocked",
  "switch": "border-st-pending text-fg-3",
  "loop": "border-st-pending text-fg-3",
  "for-each": "border-st-pending text-fg-3",
};

export const STATUS_BORDER: Record<NodeStatus, string> = {
  pending: "border-st-pending",
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
