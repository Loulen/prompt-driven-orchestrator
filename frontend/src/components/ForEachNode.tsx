import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_BORDER, STATUS_BG, STATUS_DOT } from "../nodeStyles";
import PortRow from "./PortRow";

interface ForEachPort {
  name: string;
  kind: "input" | "output";
  side: PortSide;
}

interface ForEachEditData {
  label: string;
  nodeId: string;
  ports: ForEachPort[];
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

  const inputs = data.ports.filter((p) => p.kind === "input");
  const outputs = data.ports.filter((p) => p.kind === "output");

  return (
    <div
      className={`rounded-md border-l-[3px] border-[var(--color-foreach-tint,#a78bfa)] bg-bg-4 px-3 py-2 ${
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
            nodeType="for-each"
          />
        ))}
      </div>
      <div className="flex items-center gap-2">
        <span className="text-[var(--color-foreach-tint,#a78bfa)]" style={{ fontSize: "13px" }}>
          <ForEachIcon />
        </span>
        <span className="font-medium text-fg">{data.label}</span>
        <span
          className="ml-auto rounded border border-[var(--color-foreach-tint,#a78bfa)] text-[var(--color-foreach-tint,#a78bfa)] px-1 py-px"
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          foreach
        </span>
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
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
            nodeType="for-each"
          />
        ))}
      </div>
    </div>
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

export function ForEachRunNode({ data }: NodeProps<Node<ForEachRunData>>) {
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
            nodeType="for-each"
          />
        ))}
      </div>
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <span className="text-[var(--color-foreach-tint,#a78bfa)]" style={{ fontSize: "13px" }}>
          <ForEachIcon />
        </span>
        <span className="font-medium text-fg">{data.label}</span>
        <span
          className="ml-auto rounded border border-[var(--color-foreach-tint,#a78bfa)] text-[var(--color-foreach-tint,#a78bfa)] px-1 py-px"
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          foreach
        </span>
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
      <div className="mt-1 flex flex-col gap-0.5">
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
      </div>
    </div>
  );
}
