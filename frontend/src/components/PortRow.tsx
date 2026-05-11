import type { ReactNode } from "react";
import type { NodeType, PortSide } from "../types";
import { getPortDescription } from "../portDescriptions";
import { Tooltip } from "./ui/tooltip";
import PortPill from "./PortPill";

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
