import { useState } from "react";
import { createPortal } from "react-dom";
import { Handle, Position } from "@xyflow/react";
import type { PortSide } from "../types";

const SIDE_TO_POSITION: Record<PortSide, Position> = {
  left: Position.Left,
  right: Position.Right,
  top: Position.Top,
  bottom: Position.Bottom,
};

/** Offset (px) from the cursor so the floating label never sits over the dot's
 * drag hit-area. */
const LABEL_OFFSET_X = 12;
const LABEL_OFFSET_Y = 12;

interface OutputPortDotProps {
  id: string;
  side: PortSide;
  index: number;
  total: number;
}

/**
 * Output port rendered as a plain filled dot (#170). No permanent label; the
 * port name surfaces only on hover as a cursor-relative floating label. The
 * whole dot is the xyflow source Handle, so an edge can be dragged from it.
 */
export default function OutputPortDot({
  id,
  side,
  index,
  total,
}: OutputPortDotProps) {
  const position = SIDE_TO_POSITION[side];
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);

  const pct = total === 1 ? 50 : ((index + 1) / (total + 1)) * 100;
  const isVerticalSide = side === "left" || side === "right";
  const offsetStyle: React.CSSProperties = isVerticalSide
    ? { top: `${pct}%` }
    : { left: `${pct}%` };

  return (
    <>
      <Handle
        id={id}
        type="source"
        position={position}
        className={`port-dot side-${side}`}
        style={offsetStyle}
        onPointerEnter={(e) => setCursor({ x: e.clientX, y: e.clientY })}
        onPointerMove={(e) => setCursor({ x: e.clientX, y: e.clientY })}
        onPointerLeave={() => setCursor(null)}
      />
      {cursor &&
        /* Portal to <body> so `position: fixed` resolves against the real
         * viewport. Rendered inline, the label lives inside React Flow's
         * `.react-flow__viewport`, whose pan/zoom `transform` becomes the
         * containing block for fixed positioning and displaces/scales the
         * label by the viewport matrix (#174). */
        createPortal(
          <span
            className="port-dot-lbl"
            style={{
              position: "fixed",
              left: cursor.x + LABEL_OFFSET_X,
              top: cursor.y + LABEL_OFFSET_Y,
              pointerEvents: "none",
            }}
          >
            {id}
          </span>,
          document.body,
        )}
    </>
  );
}
