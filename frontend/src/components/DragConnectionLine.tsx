import { useConnection, type ConnectionLineComponentProps } from "@xyflow/react";
import { connectionPreviewPath } from "../lib/edgePath";

export default function DragConnectionLine({
  fromX,
  fromY,
  toX,
  toY,
  connectionLineStyle,
}: ConnectionLineComponentProps) {
  const connection = useConnection();

  // Orthogonal preview (#169): the dangling wire matches the right-angle final
  // edge instead of a bezier curve.
  const path = connectionPreviewPath({ x: fromX, y: fromY }, { x: toX, y: toY });

  const sourcePortName = connection.fromHandle?.id ?? "out";
  const targetPortName = connection.toHandle?.id;

  const label = targetPortName
    ? `out/${sourcePortName} → in/${targetPortName}`
    : `out/${sourcePortName}`;

  const labelX = toX;
  const labelY = toY - 16;

  return (
    <g data-testid="drag-connection-line">
      <path
        d={path}
        fill="none"
        stroke="var(--color-acc, #10b981)"
        strokeWidth={1.5}
        style={connectionLineStyle}
      />
      <rect
        x={labelX - 4}
        y={labelY - 10}
        width={label.length * 6.2 + 8}
        height={16}
        rx={3}
        fill="var(--color-bg-3, #1a1e25)"
        stroke="var(--color-line, #2c323d)"
        strokeWidth={0.5}
      />
      <text
        x={labelX}
        y={labelY}
        fill="var(--color-acc, #10b981)"
        fontSize={10}
        fontFamily="var(--font-mono, 'Geist Mono', monospace)"
        data-testid="drag-label-text"
      >
        {label}
      </text>
    </g>
  );
}
