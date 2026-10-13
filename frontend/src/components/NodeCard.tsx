import type { NodeStatus } from "../types";
import { STATUS_BORDER, STATUS_BG, SELECTION_RING_STYLE } from "../nodeStyles";
import StatusIcon from "./StatusIcon";

interface NodeCardProps {
  status: NodeStatus;
  selected?: boolean;
  className?: string;
  style?: React.CSSProperties;
  children: React.ReactNode;
}

export function NodeCard({ status, selected, className, style, children }: NodeCardProps) {
  const borderColor = STATUS_BORDER[status];
  const bgColor = STATUS_BG[status];

  return (
    <div
      data-testid="node-card"
      className={`relative rounded-md border-[1.5px] ${borderColor} ${bgColor} px-3 py-2 ${className ?? ""}`}
      style={{
        ...(selected ? SELECTION_RING_STYLE : undefined),
        ...style,
      }}
    >
      {children}
      {/* PDO-22 (#1000): the Status icon in the corner, on a bg-1 pastille so it
          reads on any status fill, in both themes (where the failed badge sat).
          Pending wears none — nor does a library template, whose nodes have no
          run status and so read as pending. */}
      {status !== "pending" && (
        <span
          data-testid="node-status-badge"
          className="absolute z-[3] grid place-items-center rounded-full bg-bg-1"
          style={{ top: -9, right: -9, width: 20, height: 20 }}
        >
          <StatusIcon status={status} size="md" testId="node-status-icon" />
        </span>
      )}
    </div>
  );
}
