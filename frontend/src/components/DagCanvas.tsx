import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { Trash2, Terminal, Info, Play, Square } from "lucide-react";
import { isLiveRun, type NodeStatus, type NodeType, type PipelineDef, type PipelineDetail, type RunState, type RunStatus, type PortBrief } from "../types";
import { cleanupRun, attachManager, fetchRunPipeline, saveRunPipeline } from "../api";
import { serializePipeline } from "../stores/editStore";

export const START_NODE_OFFSET_X_PX = 180;

// deriveNodes places non-start/end nodes at canvas (view_x + START_NODE_OFFSET_X_PX)
// to leave room for the start node. When persisting a drag back to YAML we must
// reverse that offset so the round-trip is stable.
export function canvasToYamlX(type: string | undefined, canvasX: number): number {
  return type === "start" || type === "end"
    ? canvasX
    : canvasX - START_NODE_OFFSET_X_PX;
}

export function withUpdatedNodeView(
  pipeline: PipelineDef,
  nodeId: string,
  x: number,
  y: number,
): PipelineDef | null {
  const idx = pipeline.nodes.findIndex((n) => n.id === nodeId);
  if (idx < 0) return null;
  const updated = pipeline.nodes.slice();
  updated[idx] = {
    ...updated[idx],
    view: { x: Math.round(x), y: Math.round(y) },
  };
  return { ...pipeline, nodes: updated };
}
import { Tooltip } from "./ui/tooltip";
import { formatWhenClause } from "../predicates";
import { STATUS_BORDER, STATUS_DOT } from "../nodeStyles";
import { NodeTypeIcon, CodeDocMarker } from "./NodeTypeIcon";
import { computeBodySubgraph } from "../loopBodySubgraph";
import CleanupConfirmModal from "./CleanupConfirmModal";
import PortPill from "./PortPill";
import PortRow from "./PortRow";
import { NodeCard } from "./NodeCard";
import { SwitchRunNode } from "./SwitchNode";
import { LoopRunNode } from "./LoopNode";
import { ForEachRunNode } from "./ForEachNode";
import { MergeRunNode } from "./MergeNode";


const RUN_STATUS_DOTS: Record<RunStatus, string> = {
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
  halted: "bg-st-blocked",
  paused: "bg-st-paused",
  archived: "bg-st-archived",
};

interface PipelineNodeData {
  label: string;
  nodeId: string;
  status: NodeStatus;
  nodeType: NodeType;
  inputs: PortBrief[];
  outputs: PortBrief[];
  iter: number;
  [key: string]: unknown;
}

function PipelineNode({ data, selected }: NodeProps<Node<PipelineNodeData>>) {
  const dotColor = STATUS_DOT[data.status];

  return (
    <NodeCard status={data.status} selected={selected} style={{ minWidth: 160, fontSize: "12px" }}>
      {data.inputs.map((port, i) => (
        <PortRow
          key={`in-${port.name}`}
          portName={port.name}
          kind="input"
          side={port.side}
          index={i}
          total={data.inputs.length}
          description={port.description}
        />
      ))}
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <NodeTypeIcon type={data.nodeType} size={14} className="shrink-0 text-fg-3" />
        <span className="font-medium text-fg">{data.label}</span>
        {data.iter > 1 && (
          <span
            className="rounded bg-bg-4 px-1 font-mono text-fg-4"
            style={{ fontSize: "9px" }}
          >
            iter {data.iter}
          </span>
        )}
        <CodeDocMarker type={data.nodeType} />
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-fg-4" style={{ fontSize: "10px" }}>
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>{data.nodeId}</span>
      </div>
      {data.outputs.map((port, i) => (
        <PortRow
          key={`out-${port.name}`}
          portName={port.name}
          kind="output"
          side={port.side}
          index={i}
          total={data.outputs.length}
          description={port.description}
        />
      ))}
    </NodeCard>
  );
}

interface EndNodeData {
  inputs: PortBrief[];
  [key: string]: unknown;
}

function EndNode({ data }: NodeProps<Node<EndNodeData>>) {
  const inputs = data.inputs ?? [];
  return (
    <div
      className="grid place-items-center rounded-full border"
      style={{
        width: 28,
        height: 28,
        borderColor: "var(--color-st-blocked, #f97316)",
        color: "var(--color-st-blocked, #f97316)",
        background: "var(--color-bg-3, #1e1f23)",
      }}
    >
      {inputs.map((port, i) => (
        <PortPill
          key={`in-${port.name}`}
          id={port.name}
          kind="input"
          side={port.side}
          label={port.name}
          index={i}
          total={inputs.length}
        />
      ))}
      <Square data-testid="node-icon-end" size={12} />
    </div>
  );
}

interface StartNodeData {
  outputs: PortBrief[];
  [key: string]: unknown;
}

function StartNode({ data }: NodeProps<Node<StartNodeData>>) {
  const outputs = data.outputs ?? [];
  return (
    <div
      className="start-node grid place-items-center rounded-full border-2"
      style={{
        width: 32,
        height: 32,
        borderColor: "var(--color-acc, #10b981)",
        color: "var(--color-acc, #10b981)",
        background: "var(--color-bg-3, #1e1f23)",
      }}
    >
      <Play data-testid="node-icon-start" size={14} />
      {outputs.map((port, i) => (
        <PortPill
          key={`out-${port.name}`}
          id={port.name}
          kind="output"
          side={port.side}
          label={port.name}
          index={i}
          total={outputs.length}
        />
      ))}
    </div>
  );
}

interface MergeResolverNodeData {
  status: NodeStatus;
  conflictingNodeId: string;
  [key: string]: unknown;
}

function MergeResolverNode({ data }: NodeProps<Node<MergeResolverNodeData>>) {
  const dotColor = STATUS_DOT[data.status];
  const borderColor = STATUS_BORDER[data.status];
  return (
    <div
      className={`rounded-md border-2 border-dashed ${borderColor} bg-bg-3 px-3 py-2`}
      style={{ minWidth: 170, fontSize: "12px" }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="!bg-fg-4 !border-line !w-2 !h-2"
      />
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dotColor} ${
            data.status === "running" ? "animate-pulse" : ""
          }`}
        />
        <span className="font-medium text-fg">Merge Resolver</span>
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-fg-4" style={{ fontSize: "10px" }}>
        <span>{data.status}</span>
        <span className="font-mono" style={{ fontSize: "9px" }}>conflict: {data.conflictingNodeId}</span>
      </div>
    </div>
  );
}

interface LoopBodyOutlineData {
  width: number;
  height: number;
  loopLabel: string;
  [key: string]: unknown;
}

function LoopBodyOutlineNode({ data }: NodeProps<Node<LoopBodyOutlineData>>) {
  return (
    <div
      data-testid="loop-body-outline"
      className="pointer-events-none"
      style={{
        width: data.width,
        height: data.height,
        border: "1.5px dashed var(--color-loop-tint, #60a5fa)",
        borderRadius: 8,
        opacity: 0.35,
        background: "rgba(96, 165, 250, 0.03)",
      }}
    >
      <span
        className="absolute font-mono text-[var(--color-loop-tint,#60a5fa)]"
        style={{
          fontSize: "9px",
          opacity: 0.7,
          top: -14,
          left: 4,
        }}
      >
        {data.loopLabel} body
      </span>
    </div>
  );
}

const nodeTypes = {
  pipeline: PipelineNode,
  end: EndNode,
  start: StartNode,
  mergeResolver: MergeResolverNode,
  mergeRun: MergeRunNode,
  switchRun: SwitchRunNode,
  loopRun: LoopRunNode,
  forEachRun: ForEachRunNode,
  loopBodyOutline: LoopBodyOutlineNode,
};

const TERMINAL_STATUSES: RunStatus[] = ["completed", "failed", "halted"];

interface Props {
  run: RunState | null;
  onSelectNode: (nodeId: string | null) => void;
  selectedNodeId: string | null;
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}

const START_NODE_OFFSET_X = START_NODE_OFFSET_X_PX;

function deriveNodes(run: RunState, selectedNodeId: string | null): Node[] {
  const nodeDefs = run.node_defs ?? [];
  const nodeEntries = Object.values(run.nodes);

  const allNodes: Node[] = [];

  if (nodeDefs.length > 0) {
    const regularDefs = nodeDefs.filter(
      (d) => d.node_type !== "start" && d.node_type !== "end",
    );

    const pipelineNodes: Node[] = regularDefs.map((def, i) => {
      const nodeState = run.nodes[def.id];
      const status: NodeStatus = nodeState?.status ?? "pending";
      const iter = nodeState?.iter ?? 1;

      if (def.node_type === "switch") {
        const switchState = run.switch_states?.[def.id];
        const activeBranch: string | null = switchState?.chosen_branch ?? null;
        return {
          id: def.id,
          type: "switchRun",
          position: {
            x: (def.view_x ?? 200) + START_NODE_OFFSET_X,
            y: def.view_y ?? 80 + i * 140,
          },
          data: {
            label: def.name ?? def.id,
            nodeId: def.id,
            status,
            branches: def.outputs.map((p) => ({
              name: p.name,
              side: p.side ?? "right",
              hasWhen: false,
            })),
            inputSide: def.inputs[0]?.side ?? "left",
            activeBranch,
            iter,
          },
          selected: def.id === selectedNodeId,
        };
      }

      if (def.node_type === "loop") {
        const loopState = run.loop_states?.[def.id];
        return {
          id: def.id,
          type: "loopRun",
          position: {
            x: (def.view_x ?? 200) + START_NODE_OFFSET_X,
            y: def.view_y ?? 80 + i * 140,
          },
          data: {
            label: def.name ?? def.id,
            nodeId: def.id,
            status,
            maxIter: loopState?.max_iter ?? 5,
            currentIter: loopState?.current_iter ?? 0,
            ports: [
              ...def.inputs.map((p) => ({ name: p.name, kind: "input" as const, side: (p.side ?? "left") as import("../types").PortSide })),
              ...def.outputs.map((p) => ({ name: p.name, kind: "output" as const, side: (p.side ?? "right") as import("../types").PortSide })),
            ],
          },
          selected: def.id === selectedNodeId,
        };
      }

      if (def.node_type === "for-each") {
        const feState = run.foreach_states?.[def.id];
        return {
          id: def.id,
          type: "forEachRun",
          position: {
            x: (def.view_x ?? 200) + START_NODE_OFFSET_X,
            y: def.view_y ?? 80 + i * 140,
          },
          data: {
            label: def.name ?? def.id,
            nodeId: def.id,
            status,
            totalItems: feState?.total_items ?? 0,
            ports: [
              ...def.inputs.map((p) => ({ name: p.name, kind: "input" as const, side: (p.side ?? "left") as import("../types").PortSide })),
              ...def.outputs.map((p) => ({ name: p.name, kind: "output" as const, side: (p.side ?? "right") as import("../types").PortSide })),
            ],
          },
          selected: def.id === selectedNodeId,
        };
      }

      if (def.node_type === "merge") {
        return {
          id: def.id,
          type: "mergeRun",
          position: {
            x: (def.view_x ?? 200) + START_NODE_OFFSET_X,
            y: def.view_y ?? 80 + i * 140,
          },
          data: {
            label: def.name ?? def.id,
            nodeId: def.id,
            status,
            iter,
            inputSide: def.inputs[0]?.side ?? "left",
            outputSide: def.outputs[0]?.side ?? "right",
          },
          selected: def.id === selectedNodeId,
        };
      }

      return {
        id: def.id,
        type: "pipeline",
        position: {
          x: (def.view_x ?? 200) + START_NODE_OFFSET_X,
          y: def.view_y ?? 80 + i * 140,
        },
        data: {
          label: def.name ?? def.id,
          nodeId: def.id,
          status,
          nodeType: def.node_type,
          inputs: def.inputs,
          outputs: def.outputs,
          iter,
        },
        selected: def.id === selectedNodeId,
      };
    });

    const startDef = nodeDefs.find((d) => d.node_type === "start");
    if (startDef) {
      const targetNodeIds = run.start_node?.target_node_ids ?? [];
      const targetNodes = pipelineNodes.filter((n) =>
        targetNodeIds.includes(n.id),
      );
      const avgY =
        targetNodes.length > 0
          ? targetNodes.reduce((sum, n) => sum + n.position.y, 0) /
            targetNodes.length
          : 80;
      const minX =
        targetNodes.length > 0
          ? Math.min(...targetNodes.map((n) => n.position.x))
          : 200 + START_NODE_OFFSET_X;

      allNodes.push({
        id: startDef.id,
        type: "start",
        position: {
          x: startDef.view_x ?? minX - START_NODE_OFFSET_X,
          y: startDef.view_y ?? avgY,
        },
        data: { outputs: startDef.outputs },
        selected: startDef.id === selectedNodeId,
      });
    }

    allNodes.push(...pipelineNodes);

    const endDef = nodeDefs.find((d) => d.node_type === "end");
    if (endDef) {
      const edgeInfos = run.edges ?? [];
      const endEdges = edgeInfos.filter((ei) => ei.target_node === endDef.id);
      const sourceNodes = endEdges.map((ei) =>
        pipelineNodes.find((n) => n.id === ei.source_node),
      ).filter(Boolean) as Node[];
      const maxX =
        sourceNodes.length > 0
          ? Math.max(...sourceNodes.map((n) => n.position.x))
          : 200;
      const avgY =
        sourceNodes.length > 0
          ? sourceNodes.reduce((sum, n) => sum + n.position.y, 0) /
            sourceNodes.length
          : 80;

      allNodes.push({
        id: endDef.id,
        type: "end",
        position: {
          x: endDef.view_x ?? maxX + 280,
          y: endDef.view_y ?? avgY + 50,
        },
        data: { inputs: endDef.inputs },
        selected: endDef.id === selectedNodeId,
      });
    }
  } else {
    allNodes.push(
      ...nodeEntries.map((ns, i) => ({
        id: ns.node_id,
        type: "pipeline",
        position: { x: 200, y: 80 + i * 140 },
        data: {
          label: ns.node_id,
          nodeId: ns.node_id,
          status: ns.status,
          nodeType: "doc-only" as NodeType,
          inputs: [{ name: "in", side: "left" }],
          outputs: [{ name: "out", side: "right" }],
          iter: ns.iter,
        },
        selected: ns.node_id === selectedNodeId,
      })),
    );
  }

  if (run.merge_resolver) {
    const mr = run.merge_resolver;
    const pipelineNodes = allNodes.filter((n) => n.type === "pipeline");
    const conflictNode = pipelineNodes.find((n) => n.id === mr.conflicting_node_id);
    const cx = conflictNode?.position?.x ?? 200;
    const cy = conflictNode?.position?.y ?? 80;
    allNodes.push({
      id: "__merge_resolver__",
      type: "mergeResolver",
      position: { x: cx + 280, y: cy + 60 },
      data: {
        status: mr.status,
        conflictingNodeId: mr.conflicting_node_id,
      },
      selected: "__merge_resolver__" === selectedNodeId,
    });
  }

  const loopDefs = nodeDefs.filter((d) => d.node_type === "loop");
  const edgeInfos = run.edges ?? [];
  for (const loopDef of loopDefs) {
    const loopState = run.loop_states?.[loopDef.id];
    if (loopState?.done) continue;
    if (!loopState || loopState.current_iter < 1) continue;

    const bodyNodeIds = computeBodySubgraph(edgeInfos, nodeDefs, loopDef.id);
    if (bodyNodeIds.size === 0) continue;

    const bodyNodes = allNodes.filter((n) => bodyNodeIds.has(n.id));
    if (bodyNodes.length === 0) continue;

    const NODE_W = 180;
    const NODE_H = 70;
    const PAD = 20;

    const minX = Math.min(...bodyNodes.map((n) => n.position.x)) - PAD;
    const minY = Math.min(...bodyNodes.map((n) => n.position.y)) - PAD;
    const maxX = Math.max(...bodyNodes.map((n) => n.position.x)) + NODE_W + PAD;
    const maxY = Math.max(...bodyNodes.map((n) => n.position.y)) + NODE_H + PAD;

    allNodes.push({
      id: `__loop_body_${loopDef.id}__`,
      type: "loopBodyOutline",
      position: { x: minX, y: minY },
      data: {
        width: maxX - minX,
        height: maxY - minY,
        loopLabel: loopDef.name ?? loopDef.id,
      },
      selectable: false,
      draggable: false,
      zIndex: -1,
    });
  }

  const forEachDefs = nodeDefs.filter((d) => d.node_type === "for-each");
  for (const feDef of forEachDefs) {
    const feState = run.foreach_states?.[feDef.id];
    if (feState?.done) continue;
    if (!feState || feState.total_items < 1) continue;

    const bodyNodeIds = computeBodySubgraph(edgeInfos, nodeDefs, feDef.id);
    if (bodyNodeIds.size === 0) continue;

    const bodyNodes = allNodes.filter((n) => bodyNodeIds.has(n.id));
    if (bodyNodes.length === 0) continue;

    const NODE_W = 180;
    const NODE_H = 70;
    const PAD = 20;

    const minX = Math.min(...bodyNodes.map((n) => n.position.x)) - PAD;
    const minY = Math.min(...bodyNodes.map((n) => n.position.y)) - PAD;
    const maxX = Math.max(...bodyNodes.map((n) => n.position.x)) + NODE_W + PAD;
    const maxY = Math.max(...bodyNodes.map((n) => n.position.y)) + NODE_H + PAD;

    allNodes.push({
      id: `__foreach_body_${feDef.id}__`,
      type: "loopBodyOutline",
      position: { x: minX, y: minY },
      data: {
        width: maxX - minX,
        height: maxY - minY,
        loopLabel: feDef.name ?? feDef.id,
      },
      selectable: false,
      draggable: false,
      zIndex: -1,
    });
  }

  return allNodes;
}

function deriveEdges(run: RunState): Edge[] {
  const edgeInfos = run.edges ?? [];
  const nodeDefs = run.node_defs ?? [];
  const endNodeId = nodeDefs.find((d) => d.node_type === "end")?.id;
  const startNodeId = nodeDefs.find((d) => d.node_type === "start")?.id;

  const pipelineEdges = edgeInfos.map((ei, i) => {
    const isEndEdge = endNodeId != null && ei.target_node === endNodeId;
    const isStartEdge = startNodeId != null && ei.source_node === startNodeId;
    const isConditional = ei.when_clause != null;
    const isDashed = isEndEdge || isConditional;

    const condLabel = ei.when_clause
      ? formatWhenClause(ei.when_clause)
      : undefined;

    const label = condLabel
      ?? (ei.source_port !== ei.target_port && !isEndEdge && !isStartEdge
        ? `${ei.source_port} → ${ei.target_port}`
        : undefined);

    const strokeColor = isDashed
      ? "var(--color-st-blocked, #f97316)"
      : "var(--color-fg-4)";

    return {
      id: `e-${i}`,
      source: ei.source_node,
      target: ei.target_node,
      sourceHandle: ei.source_port || null,
      targetHandle: ei.target_port || null,
      type: "default",
      animated: !isEndEdge && run.nodes[ei.source_node]?.status === "running",
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

  const mergeResolverEdges: Edge[] = run.merge_resolver
    ? [{
        id: "e-merge-resolver",
        source: run.merge_resolver.conflicting_node_id,
        target: "__merge_resolver__",
        sourceHandle: null,
        targetHandle: null,
        type: "default",
        animated: run.merge_resolver.status === "running",
        style: {
          stroke: "var(--color-st-blocked, #f97316)",
          strokeWidth: 1.5,
          strokeDasharray: "6 3",
        },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: "var(--color-st-blocked, #f97316)",
          width: 16,
          height: 16,
        },
      }]
    : [];

  return [...pipelineEdges, ...mergeResolverEdges];
}

function DagCanvasInner({
  run,
  onSelectNode,
  selectedNodeId,
  infoOpen,
  onToggleInfo,
}: Props) {
  const [confirmCleanup, setConfirmCleanup] = useState(false);
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const runPipelineRef = useRef<PipelineDetail | null>(null);
  const runId = run?.run_id ?? null;

  const derivedNodes = useMemo(
    () => (run ? deriveNodes(run, selectedNodeId) : []),
    [run, selectedNodeId],
  );
  const derivedEdges = useMemo(
    () => (run ? deriveEdges(run) : []),
    [run],
  );

  // Merge derived nodes with current local positions so that user drags
  // are not snapped back when `run` refreshes (run mode persists drags
  // asynchronously via PUT /runs/:id/pipeline; until the round-trip lands,
  // the local position is the authoritative one for already-mounted nodes).
  useEffect(() => {
    setNodes((current) => {
      const byId = new Map(current.map((n) => [n.id, n]));
      return derivedNodes.map((dn) => {
        const existing = byId.get(dn.id);
        if (existing) return { ...dn, position: existing.position };
        return dn;
      });
    });
  }, [derivedNodes, setNodes]);

  useEffect(() => {
    setEdges(derivedEdges);
  }, [derivedEdges, setEdges]);

  // Drop the cached run pipeline when switching runs.
  useEffect(() => {
    runPipelineRef.current = null;
  }, [runId]);

  const persistNodeMove = useCallback(
    async (nodeId: string, x: number, y: number) => {
      if (!runId) return;
      let detail = runPipelineRef.current;
      if (!detail) {
        try {
          detail = await fetchRunPipeline(runId);
          runPipelineRef.current = detail;
        } catch {
          return;
        }
      }
      const updatedPipeline = withUpdatedNodeView(detail.pipeline, nodeId, x, y);
      if (!updatedPipeline) return;
      const yaml = serializePipeline(updatedPipeline);
      runPipelineRef.current = { ...detail, pipeline: updatedPipeline, yaml };
      try {
        await saveRunPipeline(runId, yaml, detail.prompts);
      } catch {
        // ignore — next user move retries with fresh pipeline state
      }
    },
    [runId],
  );

  const onNodeDragStop = useCallback(
    (_e: unknown, node: Node) => {
      void persistNodeMove(
        node.id,
        canvasToYamlX(node.type, node.position.x),
        node.position.y,
      );
    },
    [persistNodeMove],
  );

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
  const isLive = isLiveRun(run.status);
  const canCleanup = isTerminal || isLive;

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
          {onToggleInfo && (
            <Tooltip content="Pipeline info">
              <button
                data-testid="toolbar-info"
                onClick={onToggleInfo}
                className={`flex cursor-pointer items-center gap-1 rounded border px-2 py-1 transition-colors ${
                  infoOpen
                    ? "border-acc bg-acc/20 text-acc"
                    : "border-line-strong bg-bg-3 text-fg-3 hover:bg-bg-4 hover:text-fg-2"
                }`}
                style={{ fontSize: "10px" }}
              >
                <Info size={10} />
                Info
              </button>
            </Tooltip>
          )}
          <Tooltip content="Attach a terminal to the Pipeline Manager agent for this run">
            <button
              onClick={() => attachManager(run.run_id).catch(() => {})}
              className={`flex cursor-pointer items-center gap-1 rounded border px-2 py-1 transition-colors ${
                run.status === "halted"
                  ? "border-st-blocked bg-st-blocked/20 text-st-blocked hover:bg-st-blocked/30"
                  : "border-line-strong bg-bg-3 text-fg-3 hover:bg-bg-4 hover:text-fg-2"
              }`}
              style={{ fontSize: "10px" }}
            >
              <Terminal size={10} />
              Open Manager
            </button>
          </Tooltip>
          {canCleanup && (
            <Tooltip
              content={
                isLive
                  ? "Stop running sessions, then remove branches, worktrees, and artifacts. Event log is preserved."
                  : "Remove branches, worktrees, and artifacts for this run. Event log is preserved."
              }
            >
              <button
                onClick={() => setConfirmCleanup(true)}
                className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
                style={{ fontSize: "10px" }}
              >
                <Trash2 size={10} />
                {isLive ? "Stop & Cleanup" : "Cleanup"}
              </button>
            </Tooltip>
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
        onEdgeClick={(_event, edge) => onSelectNode(edge.source)}
        onPaneClick={() => onSelectNode(null)}
        onNodeDragStop={onNodeDragStop}
        deleteKeyCode={null}
        fitView
        proOptions={{ hideAttribution: true }}
        className="bg-bg-1"
      >
        <Background color="var(--color-line-soft)" gap={20} size={1} />
      </ReactFlow>

      {confirmCleanup && (
        <CleanupConfirmModal
          isLive={isLive}
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
