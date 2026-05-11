import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_DOT } from "../nodeStyles";
import { NodeCard } from "./NodeCard";
import PortRow from "./PortRow";
import { NodeTypeIcon } from "./NodeTypeIcon";
import { useIsDropTarget } from "./DragHighlightContext";

interface ForEachPort {
  name: string;
  kind: "input" | "output";
  side: PortSide;
}

interface ForEachEditData {
  label: string;
  nodeId: string;
  ports: ForEachPort[];
  status?: NodeStatus;
  [key: string]: unknown;
}

export const ForEachIcon = () => (
  <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor" stroke="none">
    <circle cx="3" cy="7" r="1.6" />
    <circle cx="9" cy="4" r="1.4" />
    <circle cx="9" cy="7" r="1.4" />
    <circle cx="9" cy="10" r="1.4" />
    <line x1="4.6" y1="7" x2="7" y2="4" stroke="currentColor" strokeWidth="1.2" />
    <line x1="4.6" y1="7" x2="7" y2="7" stroke="currentColor" strokeWidth="1.2" />
    <line x1="4.6" y1="7" x2="7" y2="10" stroke="currentColor" strokeWidth="1.2" />
  </svg>
);

export function ForEachEditNode({ data, id }: NodeProps<Node<ForEachEditData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;
  const isDropTarget = useIsDropTarget(id);

  const inputs = data.ports.filter((p) => p.kind === "input");
  const outputs = data.ports.filter((p) => p.kind === "output");

  return (
    <NodeCard status={data.status ?? "pending"} selected={isSelected} style={{ minWidth: 150, fontSize: "12px" }}>
      {inputs.map((port, i) => (
        <PortRow
          key={`in-${port.name}`}
          portName={port.name}
          kind="input"
          side={port.side}
          index={i}
          total={inputs.length}
          nodeType="for-each"
          isDrop={isDropTarget}
        />
      ))}
      <div className="flex items-center gap-2">
        <NodeTypeIcon type="for-each" size={14} className="shrink-0 text-[var(--color-foreach-tint,#a78bfa)]" />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
      </div>
      {outputs.map((port, i) => (
        <PortRow
          key={`out-${port.name}`}
          portName={port.name}
          kind="output"
          side={port.side}
          index={i}
          total={outputs.length}
          nodeType="for-each"
        />
      ))}
    </NodeCard>
  );
}

interface ForEachRunData {
  label: string;
  nodeId: string;
  status: NodeStatus;
  totalItems: number;
  ports: ForEachPort[];
  [key: string]: unknown;
}

export function ForEachRunNode({ data, selected }: NodeProps<Node<ForEachRunData>>) {
  const dotColor = STATUS_DOT[data.status];

  const inputs = data.ports.filter((p) => p.kind === "input");
  const outputs = data.ports.filter((p) => p.kind === "output");

  return (
    <NodeCard status={data.status} selected={selected} style={{ minWidth: 150, fontSize: "12px" }}>
      {inputs.map((port, i) => (
        <PortRow
          key={`in-${port.name}`}
          portName={port.name}
          kind="input"
          side={port.side}
          index={i}
          total={inputs.length}
          nodeType="for-each"
        />
      ))}
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <NodeTypeIcon type="for-each" size={14} className="shrink-0 text-[var(--color-foreach-tint,#a78bfa)]" />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-fg-4" style={{ fontSize: "10px" }}>
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>{data.nodeId}</span>
      </div>
      <div className="mt-1 flex items-center justify-center">
        <span
          data-testid="iter-badge"
          className="rounded bg-bg-3 px-2 py-0.5 font-mono text-[var(--color-foreach-tint,#a78bfa)]"
          style={{ fontSize: "10px" }}
        >
          {data.totalItems} items
        </span>
      </div>
      {outputs.map((port, i) => (
        <PortRow
          key={`out-${port.name}`}
          portName={port.name}
          kind="output"
          side={port.side}
          index={i}
          total={outputs.length}
          nodeType="for-each"
        />
      ))}
    </NodeCard>
  );
}
