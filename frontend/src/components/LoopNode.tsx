import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_DOT } from "../nodeStyles";
import { NodeCard } from "./NodeCard";
import PortRow from "./PortRow";
import { NodeTypeIcon } from "./NodeTypeIcon";
import { useIsDropTarget } from "./DragHighlightContext";

interface LoopPort {
  name: string;
  kind: "input" | "output";
  side: PortSide;
}

interface LoopEditData {
  label: string;
  nodeId: string;
  maxIter: number | string;
  ports: LoopPort[];
  status?: NodeStatus;
  [key: string]: unknown;
}

export function LoopEditNode({ data, id }: NodeProps<Node<LoopEditData>>) {
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
          nodeType="loop"
          isDrop={isDropTarget}
        />
      ))}
      <div className="flex items-center gap-2">
        <NodeTypeIcon type="loop" size={14} className="shrink-0 text-[var(--color-loop-tint,#60a5fa)]" />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
      </div>
      <div className="mt-1 flex items-center justify-center">
        <span
          data-testid="iter-badge"
          className="rounded bg-bg-3 px-2 py-0.5 font-mono text-[var(--color-loop-tint,#60a5fa)]"
          style={{ fontSize: "10px" }}
        >
          ↻ max {data.maxIter}
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
          nodeType="loop"
        />
      ))}
    </NodeCard>
  );
}

interface LoopRunData {
  label: string;
  nodeId: string;
  status: NodeStatus;
  maxIter: number | string;
  currentIter: number;
  ports: LoopPort[];
  [key: string]: unknown;
}

export function LoopRunNode({ data, selected }: NodeProps<Node<LoopRunData>>) {
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
          nodeType="loop"
        />
      ))}
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <NodeTypeIcon type="loop" size={14} className="shrink-0 text-[var(--color-loop-tint,#60a5fa)]" />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-fg-4" style={{ fontSize: "10px" }}>
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>{data.nodeId}</span>
      </div>
      <div className="mt-1 flex items-center justify-center">
        <span
          data-testid="iter-badge"
          className="rounded bg-bg-3 px-2 py-0.5 font-mono text-[var(--color-loop-tint,#60a5fa)]"
          style={{ fontSize: "10px" }}
        >
          ↻ {data.currentIter}/{data.maxIter}
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
          nodeType="loop"
        />
      ))}
    </NodeCard>
  );
}
