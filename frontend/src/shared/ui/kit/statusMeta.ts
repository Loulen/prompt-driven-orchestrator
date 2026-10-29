/** The status of a Run or of a node as the core draws it: its word, its tone, its icon. */
import {
  Archive,
  Circle,
  CircleArrowRight,
  CircleCheck,
  CircleHelp,
  CirclePause,
  CircleSlash,
  CircleStop,
  CircleX,
  Clock,
  Hourglass,
  LoaderCircle,
  type LucideIcon,
} from "lucide-react";
import type { NodeStatus } from "../../generated/NodeStatus";
import type { RunStatus } from "../../generated/RunStatus";
import type { StatusTone } from "./tones";

export interface StatusMeta {
  label: string;
  tone: StatusTone;
  icon: LucideIcon;
}

export const statusMeta = (label: string, tone: StatusTone, icon: LucideIcon): StatusMeta => ({ label, tone, icon });
const meta = statusMeta;

export const RUN_STATUS: Record<RunStatus | "stalled", StatusMeta> = {
  running: meta("Running", "running", LoaderCircle),
  awaiting_user: meta("Awaiting you", "awaiting", CircleHelp),
  completed: meta("Completed", "done", CircleCheck),
  failed: meta("Failed", "failed", CircleX),
  skipped: meta("Skipped", "skipped", CircleArrowRight),
  halted: meta("Halted", "blocked", CircleStop),
  paused: meta("Paused", "blocked", CirclePause),
  archived: meta("Archived", "skipped", Archive),
  stalled: meta("Stalled", "skipped", Hourglass),
};

export const NODE_STATUS: Record<NodeStatus, StatusMeta> = {
  pending: meta("Pending", "skipped", Circle),
  waiting: meta("Waiting", "skipped", Hourglass),
  running: meta("Running", "running", LoaderCircle),
  awaiting_user: meta("Awaiting you", "awaiting", CircleHelp),
  completed: meta("Completed", "done", CircleCheck),
  skipped: meta("Skipped", "skipped", CircleArrowRight),
  failed: meta("Failed", "failed", CircleX),
  stopped: meta("Stopped", "skipped", CircleStop),
  stale: meta("Stale", "skipped", Clock),
  // A person has to act: the awaiting colour.
  interrupted: meta("Interrupted", "awaiting", CircleSlash),
};

/** The tone a node's status is drawn in. */
export function nodeStatusTone(status: NodeStatus): StatusTone {
  return NODE_STATUS[status].tone;
}

/** The tone a Run's status is drawn in. */
export function runStatusTone(status: RunStatus | "stalled"): StatusTone {
  return RUN_STATUS[status].tone;
}
