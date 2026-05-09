import type { ReactNode } from "react";
import type { NodeType, PortSide } from "../types";
import { getPortDescription } from "../portDescriptions";
import { Tooltip } from "./ui/tooltip";
import TriangleHandle from "./TriangleHandle";

interface PortRowProps {
  portName: string;
  kind: "input" | "output";
  side: PortSide;
  index: number;
  total: number;
  nodeType?: NodeType;
  description?: string | null;
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
  children,
}: PortRowProps) {
  const tooltipContent = nodeType
    ? getPortDescription(nodeType, kind, portName, description)
    : description ?? portName;

  return (
    <Tooltip content={tooltipContent} side={side === "right" ? "right" : "left"}>
      <div
        data-testid={`port-${kind}-${portName}`}
        className="flex items-center gap-1.5 rounded bg-bg-3 px-1.5 py-0.5"
        style={{ fontSize: "10px" }}
      >
        <span className="text-fg-3">{portName}</span>
        {children}
        <TriangleHandle
          id={portName}
          kind={kind}
          side={side}
          index={index}
          total={total}
        />
      </div>
    </Tooltip>
  );
}
