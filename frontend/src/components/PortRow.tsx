import type { ReactNode } from "react";
import type { NodeType, PortSide } from "../types";
import { getPortDescription } from "../portDescriptions";
import { Tooltip } from "./ui/tooltip";
import PortPill from "./PortPill";
import OutputPortDot from "./OutputPortDot";

interface PortRowProps {
  portName: string;
  kind: "input" | "output";
  side: PortSide;
  index: number;
  total: number;
  nodeType?: NodeType;
  description?: string | null;
  isDrop?: boolean;
  children?: ReactNode;
}

export default function PortRow({
  portName,
  kind,
  side,
  index,
  total,
  nodeType,
  description,
  isDrop,
  children,
}: PortRowProps) {
  // Output ports render as plain filled dots with a cursor-relative hover label
  // instead of an always-visible pill (#170). The dot is the xyflow source
  // Handle, so an edge can be dragged straight out of it.
  if (kind === "output") {
    return (
      <div data-testid={`port-output-${portName}`} style={{ fontSize: "10px" }}>
        {children}
        <OutputPortDot id={portName} side={side} index={index} total={total} />
      </div>
    );
  }

  const tooltipContent = nodeType
    ? getPortDescription(nodeType, kind, portName, description)
    : description ?? portName;

  return (
    <Tooltip content={tooltipContent} side={side === "right" ? "right" : "left"}>
      <div
        data-testid={`port-${kind}-${portName}`}
        style={{ fontSize: "10px" }}
      >
        {children}
        <PortPill
          id={portName}
          kind={kind}
          side={side}
          label={portName}
          index={index}
          total={total}
          isDrop={isDrop}
        />
      </div>
    </Tooltip>
  );
}
