import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_BORDER, STATUS_BG, STATUS_DOT } from "../nodeStyles";
import PortRow from "./PortRow";

interface MergeEditData {
  label: string;
  nodeId: string;
  inputSide: PortSide;
  outputSide: PortSide;
  [key: string]: unknown;
}

export function MergeEditNode({ data, id }: NodeProps<Node<MergeEditData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;

  return (
    <div
      className={`rounded-md border-l-[3px] border-acc bg-bg-4 px-3 py-2 ${
        isSelected ? "ring-1 ring-acc" : ""
      }`}
      style={{ minWidth: 140, fontSize: "12px" }}
    >
      <div className="flex flex-col gap-0.5 mb-1">
        <PortRow
          portName="branches"
          kind="input"
          side={data.inputSide}
          index={0}
          total={1}
          nodeType="merge"
        />
      </div>
      <div className="flex items-center gap-2">
        <span className="h-2 w-2 shrink-0 rounded-full bg-acc" />
        <span className="font-medium text-fg">{data.label}</span>
        <span
          className="ml-auto rounded border border-acc text-acc px-1 py-px"
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          merge
        </span>
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
      </div>
      <div className="mt-1 flex flex-col gap-0.5">
        <PortRow
          portName="merged"
          kind="output"
          side={data.outputSide}
          index={0}
          total={1}
          nodeType="merge"
        />
      </div>
    </div>
  );
}

interface MergeRunData {
  label: string;
  nodeId: string;
  status: NodeStatus;
  iter: number;
  inputSide: PortSide;
  outputSide: PortSide;
  [key: string]: unknown;
}

export function MergeRunNode({ data }: NodeProps<Node<MergeRunData>>) {
  const borderColor = STATUS_BORDER[data.status];
  const bgColor = STATUS_BG[data.status];
  const dotColor = STATUS_DOT[data.status];

  return (
    <div
      className={`rounded-md border-l-[3px] ${borderColor} ${bgColor} px-3 py-2`}
      style={{ minWidth: 140, fontSize: "12px" }}
    >
      <div className="flex flex-col gap-0.5 mb-1">
        <PortRow
          portName="branches"
          kind="input"
          side={data.inputSide}
          index={0}
          total={1}
          nodeType="merge"
        />
      </div>
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <span className="font-medium text-fg">{data.label}</span>
        {data.iter > 1 && (
          <span
            className="rounded bg-bg-4 px-1 font-mono text-fg-4"
            style={{ fontSize: "9px" }}
          >
            iter {data.iter}
          </span>
        )}
        <span
          className="ml-auto rounded border border-acc text-acc px-1 py-px"
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          merge
        </span>
      </div>
      <div
        className="mt-0.5 flex items-center gap-2 text-fg-4"
        style={{ fontSize: "10px" }}
      >
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>
          {data.nodeId}
        </span>
      </div>
      <div className="mt-1 flex flex-col gap-0.5">
        <PortRow
          portName="merged"
          kind="output"
          side={data.outputSide}
          index={0}
          total={1}
          nodeType="merge"
        />
      </div>
    </div>
  );
}
