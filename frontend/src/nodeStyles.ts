import type { NodeType } from "./types";

export const TYPE_LABELS: Record<NodeType, string> = {
  "doc-only": "doc",
  "code-mutating": "code",
  "start": "start",
  "end": "end",
  "switch": "switch",
  "loop": "loop",
};

export const TYPE_COLORS: Record<NodeType, string> = {
  "doc-only": "border-st-pending text-fg-3",
  "code-mutating": "border-acc text-acc",
  "start": "border-acc text-acc",
  "end": "border-st-blocked text-st-blocked",
  "switch": "border-st-pending text-fg-3",
  "loop": "border-st-pending text-fg-3",
};
