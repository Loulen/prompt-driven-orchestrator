import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus, PortSide } from "../types";
import { useEditStore } from "../stores/editStore";
import { STATUS_DOT } from "../nodeStyles";
import { NodeCard } from "./NodeCard";
import PortPill from "./PortPill";
import PortRow from "./PortRow";
import { NodeTypeIcon } from "./NodeTypeIcon";
import { useIsDropTarget } from "./DragHighlightContext";

interface SwitchBranch {
  name: string;
  side: PortSide;
  hasWhen: boolean;
}

interface SwitchEditData {
  label: string;
  nodeId: string;
  branches: SwitchBranch[];
  inputSide: PortSide;
  status?: NodeStatus;
  [key: string]: unknown;
}

export function SwitchEditNode({ data, id }: NodeProps<Node<SwitchEditData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;
  const isDropTarget = useIsDropTarget(id);

  return (
    <NodeCard status={data.status ?? "pending"} selected={isSelected} style={{ minWidth: 140, fontSize: "12px" }}>
      <PortRow
        portName="in"
        kind="input"
        side={data.inputSide}
        index={0}
        total={1}
        nodeType="switch"
        isDrop={isDropTarget}
      />
      <div className="flex items-center gap-2">
        <NodeTypeIcon type="switch" size={14} className="shrink-0 text-[var(--color-switch-tint,#a78bfa)]" />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
      </div>
      {data.branches.map((branch, i) => (
        <PortRow
          key={branch.name}
          portName={branch.name}
          kind="output"
          side={branch.side}
          index={i}
          total={data.branches.length}
          nodeType="switch"
        >
          {!branch.hasWhen && branch.name === "default" && (
            <span className="ml-auto rounded bg-fg-4/20 px-1 text-fg-4" style={{ fontSize: "8px" }}>
              else
            </span>
          )}
        </PortRow>
      ))}
    </NodeCard>
  );
}

interface SwitchRunData {
  label: string;
  nodeId: string;
  status: NodeStatus;
  branches: SwitchBranch[];
  inputSide: PortSide;
  activeBranch: string | null;
  iter: number;
  [key: string]: unknown;
}

export function SwitchRunNode({ data, selected }: NodeProps<Node<SwitchRunData>>) {
  const dotColor = STATUS_DOT[data.status];

  return (
    <NodeCard status={data.status} selected={selected} style={{ minWidth: 140, fontSize: "12px" }}>
      <PortRow
        portName="in"
        kind="input"
        side={data.inputSide}
        index={0}
        total={1}
        nodeType="switch"
      />
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <NodeTypeIcon type="switch" size={14} className="shrink-0 text-[var(--color-switch-tint,#a78bfa)]" />
        <span className="font-medium text-fg">{data.label}</span>
        {data.iter > 1 && (
          <span className="rounded bg-bg-4 px-1 font-mono text-fg-4" style={{ fontSize: "9px" }}>
            iter {data.iter}
          </span>
        )}
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-fg-4" style={{ fontSize: "10px" }}>
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>{data.nodeId}</span>
      </div>
      <div className="mt-1 flex flex-col gap-0.5">
        {data.branches.map((branch, i) => {
          const isActive = data.activeBranch === branch.name;
          const isDimmed = data.activeBranch != null && !isActive;
          return (
            <div
              key={branch.name}
              data-testid={`branch-${branch.name}`}
              className={`flex items-center gap-1.5 rounded px-1.5 py-0.5 transition-opacity ${
                isActive
                  ? "bg-acc-bg ring-1 ring-acc/40"
                  : isDimmed
                    ? "bg-bg-3 opacity-40"
                    : "bg-bg-3"
              }`}
              style={{ fontSize: "10px" }}
            >
              <span className={isActive ? "text-acc font-medium" : "text-fg-3"}>
                {branch.name}
              </span>
              {!branch.hasWhen && branch.name === "default" && (
                <span className="ml-auto rounded bg-fg-4/20 px-1 text-fg-4" style={{ fontSize: "8px" }}>
                  else
                </span>
              )}
              <PortPill
                id={branch.name}
                kind="output"
                side={branch.side}
                label={branch.name}
                index={i}
                total={data.branches.length}
              />
            </div>
          );
        })}
      </div>
    </NodeCard>
  );
}
