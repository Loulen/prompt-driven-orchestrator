import { getBezierPath, useConnection, type ConnectionLineComponentProps } from "@xyflow/react";

export default function DragConnectionLine({
  fromX,
  fromY,
  toX,
  toY,
  fromPosition,
  toPosition,
  connectionLineStyle,
}: ConnectionLineComponentProps) {
  const connection = useConnection();

  const [path] = getBezierPath({
    sourceX: fromX,
    sourceY: fromY,
    sourcePosition: fromPosition,
    targetX: toX,
    targetY: toY,
    targetPosition: toPosition,
  });

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
