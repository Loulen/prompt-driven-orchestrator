import { useEffect, useMemo, useState } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type NodeProps,
  Handle,
  Position,
  MarkerType,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Pencil, Trash2, Terminal } from "lucide-react";
import type { NodeStatus, NodeType, RunState, RunStatus, StartNodeInfo } from "../types";
import { cleanupRun, attachManager } from "../api";
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
  halted: "bg-st-blocked",
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
  nodeId: string;
  status: NodeStatus;
  nodeType: NodeType;
  inputCount: number;
  outputCount: number;
  iter: number;
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
        {data.iter > 1 && (
          <span
            className="rounded bg-bg-4 px-1 font-mono text-fg-4"
            style={{ fontSize: "9px" }}
          >
            iter {data.iter}
          </span>
        )}
        <span
          className={`ml-auto rounded border ${typeColor} px-1 py-px`}
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          {typeLabel}
        </span>
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-fg-4" style={{ fontSize: "10px" }}>
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>{data.nodeId}</span>
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

interface HaltNodeData {
  [key: string]: unknown;
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
function HaltNode(_props: NodeProps<Node<HaltNodeData>>) {
  return (
    <div
      className="grid place-items-center rounded-full border font-mono font-bold"
      style={{
        width: 28,
        height: 28,
        borderColor: "var(--color-st-blocked, #f97316)",
        color: "var(--color-st-blocked, #f97316)",
        background: "var(--color-bg-3, #1e1f23)",
        fontSize: "11px",
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="!bg-fg-4 !border-line !w-2 !h-2"
      />
      &#x25CC;
    </div>
  );
}

interface StartNodeData {
  [key: string]: unknown;
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
function StartNode(_props: NodeProps<Node<StartNodeData>>) {
  return (
    <div
      className="start-node grid place-items-center rounded-full border-2 font-mono font-semibold"
      style={{
        width: 32,
        height: 32,
        borderColor: "var(--color-acc, #10b981)",
        color: "var(--color-acc, #10b981)",
        background: "var(--color-bg-3, #1e1f23)",
        fontSize: "14px",
      }}
    >
      &#x25B6;
      <Handle
        type="source"
        position={Position.Right}
        className="!bg-fg-4 !border-line !w-2 !h-2"
      />
    </div>
  );
}

const nodeTypes = { pipeline: PipelineNode, halt: HaltNode, start: StartNode };

const OP_SYMBOLS: Record<string, string> = {
  eq: "=", neq: "!=", lt: "<", lte: "<=", gt: ">", gte: ">=",
  in: "in", not_in: "not in",
};

function formatWhenClause(when: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const [field, predicate] of Object.entries(when)) {
    if (field === "any") continue;
    if (typeof predicate === "object" && predicate !== null) {
      for (const [op, val] of Object.entries(predicate as Record<string, unknown>)) {
        const symbol = OP_SYMBOLS[op] ?? op;
        const valStr = Array.isArray(val) ? `[${val.join(", ")}]` : String(val);
        parts.push(`${field} ${symbol} ${valStr}`);
      }
    }
  }
  return parts.join(" & ");
}

const TERMINAL_STATUSES: RunStatus[] = ["completed", "failed", "halted"];

interface Props {
  run: RunState | null;
  onSelectNode: (nodeId: string | null) => void;
  selectedNodeId: string | null;
  onToggleEdit?: (runId: string) => void;
}

const START_NODE_OFFSET_X = 180;

function deriveNodes(run: RunState, selectedNodeId: string | null): Node[] {
  const nodeDefs = run.node_defs ?? [];
  const nodeEntries = Object.values(run.nodes);
  const hasStart = run.start_node != null;
  const shiftX = hasStart ? START_NODE_OFFSET_X : 0;

  const nodes: Node[] = nodeDefs.length > 0
    ? nodeDefs.map((def, i) => {
        const nodeState = run.nodes[def.id];
        const status: NodeStatus = nodeState?.status ?? "pending";
        const iter = nodeState?.iter ?? 1;
        return {
          id: def.id,
          type: "pipeline",
          position: {
            x: (def.view_x ?? 200) + shiftX,
            y: def.view_y ?? 80 + i * 140,
          },
          data: {
            label: def.name ?? def.id,
            nodeId: def.id,
            status,
            nodeType: def.node_type,
            inputCount: def.inputs.length,
            outputCount: def.outputs.length,
            iter,
          },
          selected: def.id === selectedNodeId,
        };
      })
    : nodeEntries.map((ns, i) => ({
        id: ns.node_id,
        type: "pipeline",
        position: { x: 200 + shiftX, y: 80 + i * 140 },
        data: {
          label: ns.node_id,
          nodeId: ns.node_id,
          status: ns.status,
          nodeType: "doc-only" as NodeType,
          inputCount: 1,
          outputCount: 1,
          iter: ns.iter,
        },
        selected: ns.node_id === selectedNodeId,
      }));

  const startNodes: Node[] = [];
  if (run.start_node) {
    const targetNodes = nodes.filter((n) =>
      run.start_node!.target_node_ids.includes(n.id),
    );
    const avgY =
      targetNodes.length > 0
        ? targetNodes.reduce((sum, n) => sum + n.position.y, 0) /
          targetNodes.length
        : 80;
    const minX = targetNodes.length > 0
      ? Math.min(...targetNodes.map((n) => n.position.x))
      : 200 + shiftX;

    startNodes.push({
      id: "__start",
      type: "start",
      position: { x: minX - START_NODE_OFFSET_X, y: avgY },
      data: {},
      selected: "__start" === selectedNodeId,
    });
  }

  const edgeInfos = run.edges ?? [];
  const haltEdges = edgeInfos.filter((ei) => ei.target_node === "__halt__");
  const haltNodes: Node[] = haltEdges.map((ei, i) => {
    const sourceNode = nodes.find((n) => n.id === ei.source_node);
    const sx = sourceNode?.position?.x ?? 200;
    const sy = sourceNode?.position?.y ?? 80;
    return {
      id: `__halt__${i}`,
      type: "halt",
      position: { x: sx + 280, y: sy + 50 + i * 60 },
      data: {},
      selectable: false,
    };
  });

  return [...startNodes, ...nodes, ...haltNodes];
}

function deriveStartEdges(startNode: StartNodeInfo): Edge[] {
  return startNode.target_node_ids.map((targetId, i) => ({
    id: `start-e-${i}`,
    source: "__start",
    target: targetId,
    sourceHandle: null,
    targetHandle: null,
    type: "default",
    animated: false,
    style: {
      stroke: "var(--color-fg-4)",
      strokeWidth: 1.5,
    },
    markerEnd: {
      type: MarkerType.ArrowClosed,
      color: "var(--color-fg-4)",
      width: 16,
      height: 16,
    },
  }));
}

function deriveEdges(run: RunState): Edge[] {
  const edgeInfos = run.edges ?? [];
  const haltEdges = edgeInfos.filter((ei) => ei.target_node === "__halt__");

  const startEdges = run.start_node ? deriveStartEdges(run.start_node) : [];

  const pipelineEdges = edgeInfos.map((ei, i) => {
    const isHalt = ei.target_node === "__halt__";
    const isConditional = ei.when_clause != null;
    const isDashed = isHalt || isConditional;
    const targetId = isHalt ? `__halt__${haltEdges.indexOf(ei)}` : ei.target_node;

    const condLabel = ei.when_clause
      ? formatWhenClause(ei.when_clause)
      : undefined;

    const label = condLabel
      ?? (ei.source_port !== ei.target_port && !isHalt
        ? `${ei.source_port} → ${ei.target_port}`
        : undefined);

    const strokeColor = isDashed
      ? "var(--color-st-blocked, #f97316)"
      : "var(--color-fg-4)";

    return {
      id: `e-${i}`,
      source: ei.source_node,
      target: targetId,
      sourceHandle: null,
      targetHandle: null,
      type: "default",
      animated: !isHalt && run.nodes[ei.source_node]?.status === "running",
      style: {
        stroke: strokeColor,
        strokeWidth: 1.5,
        strokeDasharray: isDashed ? "6 3" : undefined,
      },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: strokeColor,
        width: 16,
        height: 16,
      },
      label,
      labelStyle: {
        fill: isDashed
          ? "var(--color-st-blocked, #fdba74)"
          : "var(--color-fg-4)",
        fontSize: 10,
      },
      labelBgStyle: {
        fill: isDashed
          ? "rgba(249,115,22,0.10)"
          : "var(--color-bg-2)",
        fillOpacity: 0.9,
      },
      labelBgPadding: [4, 2] as [number, number],
    };
  });

  return [...startEdges, ...pipelineEdges];
}

function DagCanvasInner({
  run,
  onSelectNode,
  selectedNodeId,
  onToggleEdit,
}: Props) {
  const [confirmCleanup, setConfirmCleanup] = useState(false);
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  const derivedNodes = useMemo(
    () => (run ? deriveNodes(run, selectedNodeId) : []),
    [run, selectedNodeId],
  );
  const derivedEdges = useMemo(
    () => (run ? deriveEdges(run) : []),
    [run],
  );

  useEffect(() => {
    setNodes(derivedNodes);
  }, [derivedNodes, setNodes]);

  useEffect(() => {
    setEdges(derivedEdges);
  }, [derivedEdges, setEdges]);

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

  const isTerminal = TERMINAL_STATUSES.includes(run.status);

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
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <button
            onClick={() => attachManager(run.run_id).catch(() => {})}
            className={`flex items-center gap-1 rounded border px-2 py-1 transition-colors ${
              run.status === "halted"
                ? "border-st-blocked bg-st-blocked/20 text-st-blocked hover:bg-st-blocked/30"
                : "border-line-strong bg-bg-3 text-fg-3 hover:bg-bg-4 hover:text-fg-2"
            }`}
            style={{ fontSize: "10px" }}
          >
            <Terminal size={10} />
            Open Manager
          </button>
          {onToggleEdit && (
            <button
              onClick={() => onToggleEdit(run.run_id)}
              className="flex items-center gap-1 rounded border border-edit-tint bg-edit-tint/10 px-2 py-1 text-edit-tint transition-colors hover:bg-edit-tint/20"
              style={{ fontSize: "10px" }}
            >
              <Pencil size={10} />
              Edit this run
            </button>
          )}
          {isTerminal && (
            <button
              onClick={() => setConfirmCleanup(true)}
              className="flex items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
              style={{ fontSize: "10px" }}
            >
              <Trash2 size={10} />
              Cleanup
            </button>
          )}
        </div>
      </div>

      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
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

export default function DagCanvas(props: Props) {
  return (
    <ReactFlowProvider>
      <DagCanvasInner {...props} />
    </ReactFlowProvider>
  );
}
