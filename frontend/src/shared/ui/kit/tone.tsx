/**
 * Status is colour and shape, never colour alone: each of the six status tones
 * of the palette has its icon. `StatusIcon` draws a Run's or a node's status
 * the way the core does.
 */
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
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
import type { NodeStatus } from "../../generated/NodeStatus";
import type { RunStatus } from "../../generated/RunStatus";
import { TONE_VAR, toneColor, type StatusTone, type Tone } from "./tones";

const TONE_ICON: Record<Tone, LucideIcon> = {
  neutral: Circle,
  running: LoaderCircle,
  awaiting: CircleHelp,
  done: CircleCheck,
  blocked: TriangleAlert,
  failed: CircleX,
  skipped: CircleArrowRight,
};

/** The tone's icon in its colour; it spins while running. */
export function ToneIcon({ tone, size = 12 }: { tone: Tone; size?: number }) {
  const Icon = TONE_ICON[tone];
  return (
    <Icon
      size={size}
      strokeWidth={size <= 12 ? 2 : 1.5}
      color={toneColor(tone)}
      className={tone === "running" ? "d-spin" : undefined}
      aria-hidden
      style={{ flex: "none" }}
    />
  );
}

interface StatusMeta {
  label: string;
  tone: StatusTone;
  icon: LucideIcon;
}

const meta = (label: string, tone: StatusTone, icon: LucideIcon): StatusMeta => ({ label, tone, icon });

const RUN_STATUS: Record<RunStatus | "stalled", StatusMeta> = {
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

const NODE_STATUS: Record<NodeStatus, StatusMeta> = {
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

export type StatusIconProps = {
  /** 12 (`sm`, in dense rows and chips), 14 (`md`), or px. */
  size?: "sm" | "md" | number;
  /** The tooltip and accessible name; the status's word by default. */
  title?: string;
} & (
  | { kind: "run"; status: RunStatus | "stalled" }
  | { kind: "node"; status: NodeStatus }
  | { kind: "tone"; status: StatusTone; label: string }
);

/** A status as the core draws it: its icon in its tone, named. */
export function StatusIcon(props: StatusIconProps) {
  const size = props.size === "sm" ? 12 : typeof props.size === "number" ? props.size : 14;
  const m =
    props.kind === "run"
      ? RUN_STATUS[props.status]
      : props.kind === "node"
        ? NODE_STATUS[props.status]
        : meta(props.label, props.status, TONE_ICON[props.status]);
  const Icon = m.icon;
  const label = props.title ?? m.label;
  return (
    <Icon
      size={size}
      strokeWidth={size <= 12 ? 2 : 1.5}
      color={TONE_VAR[m.tone]}
      className={m.tone === "running" && Icon === LoaderCircle ? "d-spin" : undefined}
      style={{ flex: "none" }}
      role="img"
      aria-label={label}
    >
      <title>{label}</title>
    </Icon>
  );
}
