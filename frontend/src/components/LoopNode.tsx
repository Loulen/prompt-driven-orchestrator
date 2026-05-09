import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_BORDER, STATUS_BG, STATUS_DOT } from "../nodeStyles";
import PortRow from "./PortRow";

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
      <div className="flex flex-col gap-0.5 mb-1">
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
      </div>
      <div className="flex items-center gap-2">
        <span className="text-[var(--color-loop-tint,#60a5fa)]" style={{ fontSize: "13px" }}>
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <path d="M11 5.5a4.5 4.5 0 1 0-1.3 3.2" />
            <path d="M11 2.5v3h-3" />
          </svg>
        </span>
        <span className="font-medium text-fg">{data.label}</span>
        <span
          className="ml-auto rounded border border-[var(--color-loop-tint,#60a5fa)] text-[var(--color-loop-tint,#60a5fa)] px-1 py-px"
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          loop
        </span>
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
      <div className="mt-1 flex flex-col gap-0.5">
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
      </div>
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
      <div className="flex flex-col gap-0.5 mb-1">
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
      </div>
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <span className="text-[var(--color-loop-tint,#60a5fa)]" style={{ fontSize: "13px" }}>
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <path d="M11 5.5a4.5 4.5 0 1 0-1.3 3.2" />
            <path d="M11 2.5v3h-3" />
          </svg>
        </span>
        <span className="font-medium text-fg">{data.label}</span>
        <span
          className="ml-auto rounded border border-[var(--color-loop-tint,#60a5fa)] text-[var(--color-loop-tint,#60a5fa)] px-1 py-px"
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          loop
        </span>
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
      <div className="mt-1 flex flex-col gap-0.5">
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
      </div>
    </div>
  );
}
