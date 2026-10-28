/**
 * Status is colour and shape, never colour alone: each of the six status tones
 * of the palette has its icon. `StatusIcon` draws a Run's or a node's status
 * the way the core does.
 */
import {
  Circle,
  CircleArrowRight,
  CircleCheck,
  CircleHelp,
  CircleX,
  LoaderCircle,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
import { TONE_VAR, toneColor, type StatusTone, type Tone } from "./tones";
import type { NodeStatus } from "../../generated/NodeStatus";
import type { RunStatus } from "../../generated/RunStatus";
import { NODE_STATUS, RUN_STATUS, statusMeta } from "./statusMeta";

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
        : statusMeta(props.label, props.status, TONE_ICON[props.status]);
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
