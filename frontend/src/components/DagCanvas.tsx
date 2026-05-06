import {
  ReactFlow,
  Background,
  type Node,
  type Edge,
  type NodeProps,
  Handle,
  Position,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { NodeState, NodeStatus, RunState, RunStatus } from "../types";

const STATUS_COLORS: Record<NodeStatus, string> = {
  pending: "border-st-pending",
  running: "border-st-running",
  completed: "border-st-done",
  failed: "border-st-failed",
};

const STATUS_BG: Record<NodeStatus, string> = {
  pending: "bg-bg-3",
  running: "bg-st-running-bg",
  completed: "bg-st-done-bg",
  failed: "bg-st-failed-bg",
};

const STATUS_DOTS: Record<NodeStatus, string> = {
  pending: "bg-st-pending",
  running: "bg-st-running",
  completed: "bg-st-done",
  failed: "bg-st-failed",
};

const RUN_STATUS_DOTS: Record<RunStatus, string> = {
  running: "bg-st-running",
  completed: "bg-st-done",
  failed: "bg-st-failed",
};

interface PipelineNodeData {
  label: string;
  status: NodeStatus;
  [key: string]: unknown;
}

function PipelineNode({ data }: NodeProps<Node<PipelineNodeData>>) {
  const borderColor = STATUS_COLORS[data.status];
  const bgColor = STATUS_BG[data.status];
  const dotColor = STATUS_DOTS[data.status];

  return (
    <div
      className={`rounded-md border-l-[3px] ${borderColor} ${bgColor} px-3 py-2`}
      style={{ minWidth: 150, fontSize: "12px" }}
    >
      <Handle type="target" position={Position.Top} className="!bg-fg-4 !border-line" />
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <span className="font-medium text-fg">{data.label}</span>
      </div>
      <div
        className="mt-0.5 text-fg-4"
        style={{ fontSize: "10px" }}
      >
        {data.status}
      </div>
      <Handle type="source" position={Position.Bottom} className="!bg-fg-4 !border-line" />
    </div>
  );
}

const nodeTypes = { pipeline: PipelineNode };

interface Props {
  run: RunState | null;
  onSelectNode: (nodeId: string | null) => void;
  selectedNodeId: string | null;
}

export default function DagCanvas({ run, onSelectNode, selectedNodeId }: Props) {
  if (!run) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4">
        Select a run to view its pipeline
      </div>
    );
  }

  const nodeEntries = Object.values(run.nodes);
  if (nodeEntries.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4">
        No nodes in this run
      </div>
    );
  }

  const nodes: Node<PipelineNodeData>[] = nodeEntries.map(
    (ns: NodeState, i: number) => ({
      id: ns.node_id,
      type: "pipeline",
      position: { x: 200, y: 80 + i * 120 },
      data: { label: ns.node_id, status: ns.status },
      selected: ns.node_id === selectedNodeId,
    }),
  );

  const edges: Edge[] = [];

  return (
    <div className="relative flex-1">
      {/* Run overlay */}
      <div
        className="absolute left-3 top-3 z-10 rounded-md border border-line bg-bg-2/90 px-3 py-2 backdrop-blur-sm"
        style={{ fontSize: "11px", maxWidth: 260 }}
      >
        <div className="font-medium text-fg">{run.pipeline_name}</div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {run.run_id}
        </div>
        <div className="mt-1 flex items-center gap-1.5">
          <span
            className={`h-1.5 w-1.5 rounded-full ${RUN_STATUS_DOTS[run.status]}`}
          />
          <span className="text-fg-3">{run.status}</span>
        </div>
      </div>

      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodeClick={(_event, node) => onSelectNode(node.id)}
        onPaneClick={() => onSelectNode(null)}
        fitView
        proOptions={{ hideAttribution: true }}
        className="bg-bg-1"
      >
        <Background color="var(--color-line-soft)" gap={20} size={1} />
      </ReactFlow>
    </div>
  );
}
