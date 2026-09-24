import { useState } from "react";
import { Handle, Position } from "@xyflow/react";
import type { PortSide } from "../types";
import { rimHandleId } from "../lib/anchorSide";
import { WIRE_BG } from "../lib/wiringColors";

/** Thickness of the drag-source rim, in screen px. Wide enough to aim at without
 *  swallowing the card's drag area. */
export const RIM = 9;

/**
 * FOUR source handles, one strip per side — not one ring: xyflow derives the
 * departure geometry from the handle's `Position`, so leaving by the right rim
 * must head right and by the bottom rim must head down. Together they cover the
 * whole border, which is what « n'importe quel point du bord » means in practice.
 *
 * NOTE the explicit `width: auto` / `height: auto`. xyflow's own
 * `.react-flow__handle` rule sets `width: 6px; height: 6px`, which beats plain
 * `top: 0; bottom: 0` insets. Left implicit, each strip collapses to a 6px square
 * in a corner of the card — and since xyflow anchors the edge on the HANDLE's
 * centre, every wire then leaves the card a dozen pixels off its side's middle.
 * `auto` against two opposing insets is what makes the strip stretch.
 */
const RIM_SIDES: { side: PortSide; position: Position; style: React.CSSProperties }[] = [
  { side: "top", position: Position.Top, style: { top: -1, left: -1, right: -1, width: "auto", height: RIM } },
  { side: "bottom", position: Position.Bottom, style: { bottom: -1, left: -1, right: -1, width: "auto", height: RIM } },
  { side: "left", position: Position.Left, style: { left: -1, top: -1, bottom: -1, height: "auto", width: RIM } },
  { side: "right", position: Position.Right, style: { right: -1, top: -1, bottom: -1, height: "auto", width: RIM } },
];

/**
 * The card's whole rim as the edge drag-source (#844). Replaces the per-output
 * dot: outputs are named on the edges that carry them, not on the card, and a
 * wire starts exactly where the border was pressed.
 *
 * Rendered LAST by its host and on a higher layer (`zIndex: 2`) so it wins the
 * pointer over the body target handle (`zIndex: 1`); inside the rim the card still
 * drags the node as usual. `onRimHover` lets the host arm the gesture visually (an
 * amber ring on the card).
 */
export default function NodeRimHandles({
  onRimHover,
}: {
  onRimHover?: (side: PortSide | null) => void;
}) {
  const [hovered, setHovered] = useState<PortSide | null>(null);

  const enter = (side: PortSide) => () => {
    setHovered(side);
    onRimHover?.(side);
  };
  const leave = () => {
    setHovered(null);
    onRimHover?.(null);
  };

  return (
    <>
      {RIM_SIDES.map(({ side, position, style }) => (
        <Handle
          key={`rim-${side}`}
          id={rimHandleId(side)}
          type="source"
          position={position}
          onPointerEnter={enter(side)}
          onPointerLeave={leave}
          data-testid={`rim-source-${side}`}
          style={{
            position: "absolute",
            transform: "none",
            borderRadius: 3,
            border: "none",
            background: hovered === side ? WIRE_BG : "transparent",
            cursor: "crosshair",
            zIndex: 2,
            minWidth: 0,
            minHeight: 0,
            ...style,
          }}
        />
      ))}
    </>
  );
}
