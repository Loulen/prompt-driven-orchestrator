import type { NodeStatus } from "../../shared/generated/NodeStatus";
import { nodeStatusTone } from "../../shared/ui/kit/statusMeta";

interface NodeCardProps {
  /** The node's status in a Run; `null` on a template, which has none. */
  status: NodeStatus | null;
  selected?: boolean;
  /** An on-demand node the Run never launched: greyed, dashed (ADR-0086). */
  idle?: boolean;
  /** The rim is hovered: a wire may start here (#844). */
  rimHover?: boolean;
  /** Releasing the wire here would land it (#844). */
  wireTarget?: boolean;
  className?: string;
  style?: React.CSSProperties;
  children: React.ReactNode;
}

/** A pending node has no status to show yet: no tint, no coloured border. */
const UNSTARTED: ReadonlySet<NodeStatus> = new Set(["pending", "waiting"]);

/**
 * The frame of a node on the canvas: its status as a light tint, a border and
 * (inside) an icon; the selection as one ruby ring outside the border.
 */
export function NodeCard({ status, selected, idle, rimHover, wireTarget, className, style, children }: NodeCardProps) {
  const tone = status && !UNSTARTED.has(status) ? nodeStatusTone(status) : undefined;
  return (
    <div
      data-testid="node-card"
      data-st={tone}
      data-status={status ?? undefined}
      data-selected={selected || undefined}
      data-idle={idle || undefined}
      data-rim={rimHover || undefined}
      data-wire={wireTarget || undefined}
      className={`cv-n${className ? ` ${className}` : ""}`}
      style={style}
    >
      {children}
    </div>
  );
}
