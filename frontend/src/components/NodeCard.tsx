import type { NodeStatus } from "../types";
import { STATUS_BORDER, STATUS_BG, SELECTION_RING_STYLE } from "../nodeStyles";

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
      {status === "failed" && (
        <span
          data-testid="failed-badge"
          className="absolute z-[3] rounded-full bg-st-failed border-2 border-bg-1"
          style={{
            top: -7,
            right: -7,
            width: 18,
            height: 18,
            boxShadow: "0 2px 6px rgba(239,68,68,0.4)",
          }}
        />
      )}
    </div>
  );
}
