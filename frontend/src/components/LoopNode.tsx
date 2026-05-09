import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_BORDER, STATUS_BG, STATUS_DOT } from "../nodeStyles";
import TriangleHandle from "./TriangleHandle";
import { NodeTypeIcon } from "./NodeTypeIcon";

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
  [key: string]: unknown;
}

export function LoopEditNode({ data, id }: NodeProps<Node<LoopEditData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;

  const inputs = data.ports.filter((p) => p.kind === "input");
  const outputs = data.ports.filter((p) => p.kind === "output");

  return (
    <div
      className={`rounded-md border-l-[3px] border-[var(--color-loop-tint,#60a5fa)] bg-bg-4 px-3 py-2 ${
        isSelected ? "ring-1 ring-acc" : ""
      }`}
      style={{ minWidth: 150, fontSize: "12px" }}
    >
      {inputs.map((port, i) => (
        <TriangleHandle
          key={`in-${port.name}`}
          id={port.name}
          kind="input"
          side={port.side}
          index={i}
          total={inputs.length}
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
        <TriangleHandle
          key={`out-${port.name}`}
          id={port.name}
          kind="output"
          side={port.side}
          index={i}
          total={outputs.length}
        />
      ))}
    </div>
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

export function LoopRunNode({ data }: NodeProps<Node<LoopRunData>>) {
  const borderColor = STATUS_BORDER[data.status];
  const bgColor = STATUS_BG[data.status];
  const dotColor = STATUS_DOT[data.status];

  const inputs = data.ports.filter((p) => p.kind === "input");
  const outputs = data.ports.filter((p) => p.kind === "output");

  return (
    <div
      className={`rounded-md border-l-[3px] ${borderColor} ${bgColor} px-3 py-2`}
      style={{ minWidth: 150, fontSize: "12px" }}
    >
      {inputs.map((port, i) => (
        <TriangleHandle
          key={`in-${port.name}`}
          id={port.name}
          kind="input"
          side={port.side}
          index={i}
          total={inputs.length}
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
        <TriangleHandle
          key={`out-${port.name}`}
          id={port.name}
          kind="output"
          side={port.side}
          index={i}
          total={outputs.length}
        />
      ))}
    </div>
  );
}
