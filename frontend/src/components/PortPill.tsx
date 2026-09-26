import { Handle, Position } from "@xyflow/react";
import type { PortSide } from "../shared/generated/PortSide";
import { chevronPoints } from "./chevronPoints";

const SIDE_TO_POSITION: Record<PortSide, Position> = {
  left: Position.Left,
  right: Position.Right,
  top: Position.Top,
  bottom: Position.Bottom,
};

interface PortPillProps {
  id: string;
  kind: "input" | "output";
  side: PortSide;
  label: string;
  index: number;
  total: number;
  isDrop?: boolean;
}

export default function PortPill({
  id,
  kind,
  side,
  label,
  index,
  total,
  isDrop,
}: PortPillProps) {
  const type = kind === "input" ? "target" : "source";
  const position = SIDE_TO_POSITION[side];
  const pts = chevronPoints(kind, side);

  const pct = total === 1 ? 50 : ((index + 1) / (total + 1)) * 100;
  const isVerticalSide = side === "left" || side === "right";
  const offsetStyle: React.CSSProperties = isVerticalSide
    ? { top: `${pct}%` }
    : { left: `${pct}%` };

  return (
    <Handle
      id={id}
      type={type}
      position={position}
      className={`port-pill side-${side} kind-${kind}${isDrop ? " is-drop" : ""}`}
      style={offsetStyle}
    >
      <span className="pp-chev">
        <svg width="8" height="8" viewBox="0 0 8 10">
          <polyline
            points={pts}
            stroke="currentColor"
            strokeWidth="1.4"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </span>
      <span className="pp-label">{label}</span>
    </Handle>
  );
}
