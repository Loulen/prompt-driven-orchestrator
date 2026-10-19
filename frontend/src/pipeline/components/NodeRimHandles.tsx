import { useState } from "react";
import { Handle, Position, useNodeId, useStore } from "@xyflow/react";
import type { PortSide } from "../../shared/generated/PortSide";
import { rimHandleId } from "../../lib/anchorSide";
import { WIRE_BG } from "../../lib/wiringColors";
import { CARD_HEIGHT, CARD_WIDTH } from "../../lib/nodePlacement";
import { rimBand, rimDepth, type RimBand } from "../../lib/rimBand";

/**
 * FOUR source handles, one strip per side — not one ring: xyflow derives the
 * departure geometry from the handle's `Position`, so leaving by the right rim
 * must head right and by the bottom rim must head down. Together they cover the
 * whole border, which is what « n'importe quel point du bord » means in practice.
 *
 * Each strip is 8px wide ON SCREEN at any zoom (CORE-88, `rimBand`): its flow
 * width is divided by the zoom, so a dezoomed card is as easy to pull a wire from
 * as one at 100 %. At most 3px of it lies inside the card, the rest outside, so
 * the card's middle still moves it.
 *
 * NOTE the explicit `width: auto` / `height: auto`. xyflow's own
 * `.react-flow__handle` rule sets `width: 6px; height: 6px`, which beats plain
 * `top: 0; bottom: 0` insets. Left implicit, each strip collapses to a 6px square
 * in a corner of the card — and since xyflow anchors the edge on the HANDLE's
 * centre, every wire then leaves the card a dozen pixels off its side's middle.
 * `auto` against two opposing insets is what makes the strip stretch.
 */
const RIM_SIDES: { side: PortSide; position: Position }[] = [
  { side: "top", position: Position.Top },
  { side: "bottom", position: Position.Bottom },
  { side: "left", position: Position.Left },
  { side: "right", position: Position.Right },
];

/** The strip of `side`, placed across the card's outer edge. Insets are taken
 *  from inside the card's border, so its outer edge sits at `-border`. */
function stripStyle(side: PortSide, band: RimBand, corner: number, border: number): React.CSSProperties {
  const along = { width: "auto", height: band.width } as const;
  const across = { height: "auto", width: band.width } as const;
  const out = -(band.outside + border);
  const c = -(corner + border);
  switch (side) {
    case "top":
      return { top: out, left: c, right: c, ...along };
    case "bottom":
      return { bottom: out, left: c, right: c, ...along };
    case "left":
      return { left: out, top: c, bottom: c, ...across };
    case "right":
      return { right: out, top: c, bottom: c, ...across };
  }
}

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
  isConnectable = true,
  border = 1,
}: {
  onRimHover?: (side: PortSide | null) => void;
  /** False on a locked canvas (ADR-0085): no wire can start, so no rim cue. */
  isConnectable?: boolean;
  /** The card's border width, in flow px (2 on a card awaiting its ▶). */
  border?: number;
}) {
  const [hovered, setHovered] = useState<PortSide | null>(null);
  const zoom = useStore((s) => s.transform[2]);
  const nodeId = useNodeId();
  const width = useStore((s) => (nodeId ? s.nodeLookup.get(nodeId)?.measured?.width : undefined)) ?? CARD_WIDTH;
  const height = useStore((s) => (nodeId ? s.nodeLookup.get(nodeId)?.measured?.height : undefined)) ?? CARD_HEIGHT;

  const enter = (side: PortSide) => () => {
    if (!isConnectable) return;
    setHovered(side);
    onRimHover?.(side);
  };
  const leave = () => {
    setHovered(null);
    onRimHover?.(null);
  };

  return (
    <>
      {RIM_SIDES.map(({ side, position }) => {
        const band = rimBand(zoom, rimDepth(side, { width, height }));
        // The strips run past the corners by the outside part of the band, so
        // the corner is covered as well as the sides.
        const style = stripStyle(side, band, band.outside, border);
        return (
        <Handle
          key={`rim-${side}`}
          id={rimHandleId(side)}
          type="source"
          position={position}
          isConnectable={isConnectable}
          onPointerEnter={enter(side)}
          onPointerLeave={leave}
          data-testid={`rim-source-${side}`}
          style={{
            position: "absolute",
            transform: "none",
            borderRadius: 3,
            border: "none",
            background: hovered === side ? WIRE_BG : "transparent",
            cursor: isConnectable ? "crosshair" : "default",
            zIndex: 2,
            minWidth: 0,
            minHeight: 0,
            ...style,
          }}
        />
        );
      })}
    </>
  );
}
