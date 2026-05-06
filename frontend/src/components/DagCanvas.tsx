import { useState } from "react";
import {
  ReactFlow,
  Background,
  type Node,
  type Edge,
  type NodeProps,
  Handle,
  Position,
  MarkerType,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Trash2 } from "lucide-react";
import type { NodeStatus, NodeType, RunState, RunStatus } from "../types";
import { cleanupRun } from "../api";
import CleanupConfirmModal from "./CleanupConfirmModal";

const STATUS_COLORS: Record<NodeStatus, string> = {
  pending: "border-st-pending",
  running: "border-st-running",
  awaiting_user: "border-st-await",
  completed: "border-st-done",
  failed: "border-st-failed",
};

const STATUS_BG: Record<NodeStatus, string> = {
  pending: "bg-bg-3",
  running: "bg-st-running-bg",
  awaiting_user: "bg-st-await-bg",
  completed: "bg-st-done-bg",
  failed: "bg-st-failed-bg",
};

const STATUS_DOTS: Record<NodeStatus, string> = {
  pending: "bg-st-pending",
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
};

const RUN_STATUS_DOTS: Record<RunStatus, string> = {
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
  archived: "bg-st-archived",
};

const TYPE_LABELS: Record<NodeType, string> = {
  "doc-only": "doc",
  "code-mutating": "code",
};

const TYPE_COLORS: Record<NodeType, string> = {
  "doc-only": "border-st-pending text-fg-3",
  "code-mutating": "border-acc text-acc",
};

interface PipelineNodeData {
  label: string;
  status: NodeStatus;
  nodeType: NodeType;
  inputCount: number;
  outputCount: number;
  [key: string]: unknown;
}

function PipelineNode({ data }: NodeProps<Node<PipelineNodeData>>) {
  const borderColor = STATUS_COLORS[data.status];
  const bgColor = STATUS_BG[data.status];
  const dotColor = STATUS_DOTS[data.status];
  const typeLabel = TYPE_LABELS[data.nodeType] ?? data.nodeType;
  const typeColor = TYPE_COLORS[data.nodeType] ?? TYPE_COLORS["doc-only"];

  return (
    <div
      className={`rounded-md border-l-[3px] ${borderColor} ${bgColor} px-3 py-2`}
      style={{ minWidth: 160, fontSize: "12px" }}
    >
      {data.inputCount > 0 && (
        <Handle
          type="target"
          position={Position.Left}
          className="!bg-fg-4 !border-line !w-2 !h-2"
        />
      )}
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <span className="font-medium text-fg">{data.label}</span>
        <span
          className={`ml-auto rounded border ${typeColor} px-1 py-px`}
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          {typeLabel}
        </span>
      </div>
      <div className="mt-0.5 text-fg-4" style={{ fontSize: "10px" }}>
        {data.status}
      </div>
      {data.outputCount > 0 && (
        <Handle
          type="source"
          position={Position.Right}
          className="!bg-fg-4 !border-line !w-2 !h-2"
        />
      )}
    </div>
  );
}

const nodeTypes = { pipeline: PipelineNode };

interface Props {
  run: RunState | null;
  onSelectNode: (nodeId: string | null) => void;
  selectedNodeId: string | null;
}

export default function DagCanvas({
  run,
  onSelectNode,
  selectedNodeId,
}: Props) {
  const [confirmCleanup, setConfirmCleanup] = useState(false);

  if (!run) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4">
        Select a run to view its pipeline
      </div>
    );
  }

  const nodeDefs = run.node_defs ?? [];
  const nodeEntries = Object.values(run.nodes);

  if (nodeEntries.length === 0 && nodeDefs.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4">
        No nodes in this run
      </div>
    );
  }

  // Build node list from node_defs (has position + type info) with status from run.nodes
  const nodes: Node<PipelineNodeData>[] = nodeDefs.length > 0
    ? nodeDefs.map((def, i) => {
        const nodeState = run.nodes[def.id];
        const status: NodeStatus = nodeState?.status ?? "pending";
        return {
          id: def.id,
          type: "pipeline",
          position: {
            x: def.view_x ?? 200,
            y: def.view_y ?? 80 + i * 140,
          },
          data: {
            label: def.id,
            status,
            nodeType: def.node_type,
            inputCount: def.inputs.length,
            outputCount: def.outputs.length,
          },
          selected: def.id === selectedNodeId,
        };
      })
    : nodeEntries.map((ns, i) => ({
        id: ns.node_id,
        type: "pipeline",
        position: { x: 200, y: 80 + i * 140 },
        data: {
          label: ns.node_id,
          status: ns.status,
          nodeType: "doc-only" as NodeType,
          inputCount: 1,
          outputCount: 1,
        },
        selected: ns.node_id === selectedNodeId,
      }));

  const edgeInfos = run.edges ?? [];
  const edges: Edge[] = edgeInfos.map((ei, i) => ({
    id: `e-${i}`,
    source: ei.source_node,
    target: ei.target_node,
    sourceHandle: null,
    targetHandle: null,
    type: "default",
    animated: run.nodes[ei.source_node]?.status === "running",
    style: { stroke: "var(--color-fg-4)", strokeWidth: 1.5 },
    markerEnd: {
      type: MarkerType.ArrowClosed,
      color: "var(--color-fg-4)",
      width: 16,
      height: 16,
    },
    label: ei.source_port !== ei.target_port
      ? `${ei.source_port} → ${ei.target_port}`
      : undefined,
    labelStyle: { fill: "var(--color-fg-4)", fontSize: 10 },
    labelBgStyle: { fill: "var(--color-bg-2)", fillOpacity: 0.9 },
    labelBgPadding: [4, 2] as [number, number],
  }));

  const isTerminal = run.status === "completed" || run.status === "failed";

  async function handleCleanup() {
    try {
      await cleanupRun(run!.run_id);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmCleanup(false);
  }

  return (
    <div className="relative flex-1">
      {/* Run overlay */}
      <div
        className="absolute left-3 top-3 z-10 rounded-md border border-line bg-bg-2/90 px-3 py-2 backdrop-blur-sm"
        style={{ fontSize: "11px", maxWidth: 260 }}
      >
        <div className="font-medium text-fg">{run.pipeline_name}</div>
        <div
          className="mt-0.5 font-mono text-fg-4"
          style={{ fontSize: "10px" }}
        >
          {run.run_id}
        </div>
        <div className="mt-1 flex items-center gap-1.5">
          <span
            className={`h-1.5 w-1.5 rounded-full ${RUN_STATUS_DOTS[run.status]}`}
          />
          <span className="text-fg-3">{run.status}</span>
        </div>
        {isTerminal && (
          <button
            onClick={() => setConfirmCleanup(true)}
            className="mt-2 flex items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
            style={{ fontSize: "10px" }}
          >
            <Trash2 size={10} />
            Cleanup
          </button>
        )}
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

      {confirmCleanup && (
        <CleanupConfirmModal
          onConfirm={handleCleanup}
          onCancel={() => setConfirmCleanup(false)}
        />
      )}
    </div>
  );
}
