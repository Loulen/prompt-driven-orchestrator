import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_DOT } from "../nodeStyles";
import { NodeCard } from "./NodeCard";
import PortRow from "./PortRow";
import { NodeTypeIcon } from "./NodeTypeIcon";
import { useIsDropTarget } from "./DragHighlightContext";

interface MergeEditData {
  label: string;
  nodeId: string;
  inputSide: PortSide;
  outputSide: PortSide;
  status?: NodeStatus;
  [key: string]: unknown;
}

export function MergeEditNode({ data, id }: NodeProps<Node<MergeEditData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;
  const isDropTarget = useIsDropTarget(id);

  return (
    <NodeCard status={data.status ?? "pending"} selected={isSelected} style={{ minWidth: 140, fontSize: "12px" }}>
      <PortRow
        portName="branches"
        kind="input"
        side={data.inputSide}
        index={0}
        total={1}
        nodeType="merge"
        isDrop={isDropTarget}
      />
      <div className="flex items-center gap-2">
        <NodeTypeIcon type="merge" size={14} className="shrink-0 text-acc" />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
      </div>
      <PortRow
        portName="merged"
        kind="output"
        side={data.outputSide}
        index={0}
        total={1}
        nodeType="merge"
      />
    </NodeCard>
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

export function MergeRunNode({ data, selected }: NodeProps<Node<MergeRunData>>) {
  const dotColor = STATUS_DOT[data.status];

  return (
    <NodeCard status={data.status} selected={selected} style={{ minWidth: 140, fontSize: "12px" }}>
      <PortRow
        portName="branches"
        kind="input"
        side={data.inputSide}
        index={0}
        total={1}
        nodeType="merge"
      />
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <NodeTypeIcon type="merge" size={14} className="shrink-0 text-acc" />
        <span className="font-medium text-fg">{data.label}</span>
        {data.iter > 1 && (
          <span
            className="rounded bg-bg-4 px-1 font-mono text-fg-4"
            style={{ fontSize: "9px" }}
          >
            iter {data.iter}
          </span>
        )}
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
      <PortRow
        portName="merged"
        kind="output"
        side={data.outputSide}
        index={0}
        total={1}
        nodeType="merge"
      />
    </NodeCard>
  );
}
