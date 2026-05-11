import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  ReactFlow,
  Background,
  useNodesState,
  useEdgesState,
  useReactFlow,
  type Node,
  type Edge,
  type NodeProps,
  type Connection,
  MarkerType,
  ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { NodeDef, NodeStatus, NodeType, PipelineDef, PortBrief, RunState } from "../types";
import type { LibraryEntry, LibraryPipelineEntry } from "../api";
import { deriveEditNodes } from "./editNodeDerivation";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";
import PortRow from "./PortRow";
import { NodeTypeIcon, CodeDocMarker } from "./NodeTypeIcon";
import { NodeCard } from "./NodeCard";
import { SwitchEditNode } from "./SwitchNode";
import { LoopEditNode } from "./LoopNode";
import { ForEachEditNode } from "./ForEachNode";
import { MergeEditNode } from "./MergeNode";
import EditToolbar from "./EditToolbar";
import LintBanner from "./LintBanner";
import DragConnectionLine from "./DragConnectionLine";
import { DragHighlightProvider, useIsDropTarget } from "./DragHighlightContext";
import PipelineStar from "./PipelineStar";
import { usePipelineLibraryState } from "../hooks/useLibraryPipelines";

interface EditNodeData {
  label: string;
  nodeId: string;
  nodeType: NodeType;
  inputs: PortBrief[];
  outputs: PortBrief[];
  interactive: boolean;
  status: NodeStatus;
  [key: string]: unknown;
}

function EditNode({ data, id }: NodeProps<Node<EditNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;
  const isDropTarget = useIsDropTarget(id);
  const iconColor =
    data.nodeType === "start" ? "text-acc"
    : data.nodeType === "end" ? "text-st-blocked"
    : "text-fg-3";

  return (
    <NodeCard status={data.status} selected={isSelected} style={{ minWidth: 160, fontSize: "12px" }}>
      {data.inputs.map((port, i) => (
        <PortRow
          key={`in-${port.name}`}
          portName={port.name}
          kind="input"
          side={port.side}
          index={i}
          total={data.inputs.length}
          description={port.description}
          isDrop={isDropTarget}
        />
      ))}
      <div className="flex items-center gap-2">
        <NodeTypeIcon type={data.nodeType} size={14} className={`shrink-0 ${iconColor}`} />
        <span className="font-medium text-fg">{data.label}</span>
        {data.interactive && (
          <span
            className="rounded bg-st-await-bg px-1 font-mono text-st-await"
            style={{ fontSize: "9px" }}
          >
            interactive
          </span>
        )}
        <CodeDocMarker type={data.nodeType} />
      </div>
      <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
        {data.nodeId}
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

const nodeTypes = { edit: EditNode, switch: SwitchEditNode, loop: LoopEditNode, foreach: ForEachEditNode, merge: MergeEditNode };

const DEFAULT_NODE_NAMES: Partial<Record<NodeType, string>> = {
  "code-mutating": "implementer",
  "switch": "switch",
  "loop": "loop",
  "for-each": "foreach",
  "merge": "merge",
};

function deriveEditEdges(pipeline: PipelineDef): Edge[] {
  const endNodeId = pipeline.nodes.find((n) => n.type === "end")?.id;

  return pipeline.edges.map((e, i) => {
    const isEndEdge = endNodeId != null && e.target.node === endNodeId;
    const isDashed = isEndEdge;

    const strokeColor = isDashed
      ? "var(--color-st-blocked, #f97316)"
      : "var(--color-fg-4)";

    const sourcePort = e.source.port;
    const targetPort = e.target.port;

    return {
      id: `e-${i}`,
      source: e.source.node,
      target: e.target.node,
      sourceHandle: sourcePort || null,
      targetHandle: targetPort || null,
      type: "default",
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
    };
  });
}

interface EditCanvasProps {
  libraryEntries: LibraryEntry[];
  libraryPipelines: LibraryPipelineEntry[];
  onLibraryDelete: (name: string) => void;
  onLibraryPipelinesChanged: () => void;
  infoOpen?: boolean;
  onToggleInfo?: () => void;
  onCloseInfo?: () => void;
  runState?: RunState | null;
}

function EditCanvasInner({ libraryEntries, libraryPipelines, onLibraryDelete, onLibraryPipelinesChanged, infoOpen, onToggleInfo, onCloseInfo, runState }: EditCanvasProps) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setSelection = useEditStore((s) => s.setSelection);
  const setScrollToPort = useEditStore((s) => s.setScrollToPort);
  const updateNode = useEditStore((s) => s.updateNode);
  const addEdgeToStore = useEditStore((s) => s.addEdge);
  const deleteNode = useEditStore((s) => s.deleteNode);
  const duplicateNode = useEditStore((s) => s.duplicateNode);
  const deleteEdge = useEditStore((s) => s.deleteEdge);
  const addNodeToStore = useEditStore((s) => s.addNode);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    type: "node" | "edge";
    id: string;
    edgeIndex?: number;
  } | null>(null);
  const reactFlowRef = useRef<HTMLDivElement>(null);
  const reactFlow = useReactFlow();
  const [isDraggingEdge, setIsDraggingEdge] = useState(false);
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const dragHighlightNodeId = isDraggingEdge ? hoveredNodeId : null;

  const tab = openTabs.find((t) => t.id === activeTabId);
  const pipeline = tab?.pipeline;

  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  // Only apply live run status when the active tab is the run-scoped tab
  // matching the currently loaded RunState — otherwise (template pipelines,
  // or a different run) every node stays "pending".
  const activeRunState =
    tab?.runId && runState && tab.runId === runState.run_id ? runState : null;

  const pipelineSync = usePipelineLibraryState(pipeline ?? null, libraryPipelines);

  const derivedNodes = useMemo(
    () => (pipeline ? deriveEditNodes(pipeline, activeRunState) : []),
    [pipeline, activeRunState],
  );
  const derivedEdges = useMemo(
    () => (pipeline ? deriveEditEdges(pipeline) : []),
    [pipeline],
  );

  useEffect(() => {
    setNodes(derivedNodes);
  }, [derivedNodes, setNodes]);

  useEffect(() => {
    setEdges(derivedEdges);
  }, [derivedEdges, setEdges]);

  const onConnect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target) return;
      if (!pipeline) return;
      const sourceNode = pipeline.nodes.find((n) => n.id === connection.source);
      const targetNode = pipeline.nodes.find((n) => n.id === connection.target);
      const sourcePort = connection.sourceHandle ?? sourceNode?.outputs[0]?.name ?? "out";
      const targetPort = connection.targetHandle ?? targetNode?.inputs[0]?.name ?? "in";

      addEdgeToStore({
        source: { node: connection.source, port: sourcePort },
        target: { node: connection.target, port: targetPort },
      });
    },
    [addEdgeToStore, pipeline],
  );

  const onNodeDragStop = useCallback(
    (_: unknown, node: Node) => {
      updateNode(node.id, {
        view: { x: Math.round(node.position.x), y: Math.round(node.position.y) },
      });
    },
    [updateNode],
  );

  const handleNodeContextMenu = useCallback(
    (event: React.MouseEvent, node: Node) => {
      event.preventDefault();
      const nodeDef = pipeline?.nodes.find((n) => n.id === node.id);
      if (nodeDef?.type === "start" || nodeDef?.type === "end") return;
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        type: "node",
        id: node.id,
      });
    },
    [pipeline],
  );

  const handleEdgeContextMenu = useCallback(
    (event: React.MouseEvent, edge: Edge) => {
      event.preventDefault();
      const idx = parseInt(edge.id.replace("e-", ""), 10);
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        type: "edge",
        id: edge.id,
        edgeIndex: idx,
      });
    },
    [],
  );

  const onConnectStart = useCallback(() => setIsDraggingEdge(true), []);
  const onConnectEnd = useCallback(() => {
    setIsDraggingEdge(false);
    setHoveredNodeId(null);
  }, []);
  const onNodeMouseEnter = useCallback((_: ReactMouseEvent, node: Node) => setHoveredNodeId(node.id), []);
  const onNodeMouseLeave = useCallback(() => setHoveredNodeId(null), []);

  const diagnostics = useMemo(() => {
    if (!tab) return [];
    const base = tab.diagnostics ?? [];
    const extra: string[] = [];
    for (const n of tab.pipeline.nodes) {
      if (n.type !== "for-each" || n.over) continue;
      const hasInEdge = tab.pipeline.edges.some(
        (e) => e.target.node === n.id && e.target.port === "in",
      );
      if (hasInEdge) {
        extra.push(
          `ForEach node "${n.name ?? n.id}" has an "in" edge but no "over" field set. Select the node and choose which list field to iterate.`,
        );
      }
    }
    return [...base, ...extra];
  }, [tab]);

  if (!tab || !pipeline) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4">
        Select a pipeline to edit
      </div>
    );
  }

  const computeDropPosition = (): { x: number; y: number } => {
    const wrapper = reactFlowRef.current;
    // Approximate default node-card footprint; nodes auto-size around this.
    const APPROX_W = 180;
    const APPROX_H = 80;
    let cx: number;
    let cy: number;
    if (wrapper) {
      const rect = wrapper.getBoundingClientRect();
      const flow = reactFlow.screenToFlowPosition({
        x: rect.left + rect.width / 2,
        y: rect.top + rect.height / 2,
      });
      cx = flow.x;
      cy = flow.y;
    } else {
      cx = 200;
      cy = 200;
    }
    let x = Math.round(cx - APPROX_W / 2);
    let y = Math.round(cy - APPROX_H / 2);
    // Nudge to avoid stacking new nodes on top of existing ones at the same spot.
    const existing = pipeline?.nodes ?? [];
    const THRESHOLD = 30;
    let guard = 0;
    while (
      guard++ < 20 &&
      existing.some(
        (n) =>
          n.view != null &&
          Math.abs(n.view.x - x) < THRESHOLD &&
          Math.abs(n.view.y - y) < THRESHOLD,
      )
    ) {
      x += 40;
      y += 40;
    }
    return { x, y };
  };

  const handleAddNode = (type: NodeType) => {
    const id = generateNodeId();
    const name = DEFAULT_NODE_NAMES[type] ?? "node";

    const view = computeDropPosition();
    let newNode: NodeDef;
    switch (type) {
      case "merge":
        newNode = {
          id, name, type, interactive: false, view,
          inputs: [{ name: "branches", repeated: true, side: "left" }],
          outputs: [{ name: "merged", repeated: false, side: "right" }],
        };
        break;
      case "loop":
        newNode = {
          id, name, type, interactive: false, view, max_iter: 5,
          inputs: [
            { name: "in", repeated: false, side: "left" },
            { name: "break", repeated: false, side: "left" },
          ],
          outputs: [
            { name: "body", repeated: false, side: "right" },
            { name: "done", repeated: false, side: "right" },
          ],
        };
        break;
      case "for-each":
        newNode = {
          id, name, type, interactive: false, view,
          inputs: [
            { name: "in", repeated: false, side: "left" },
            { name: "break", repeated: false, side: "left" },
          ],
          outputs: [
            { name: "body", repeated: false, side: "right" },
            { name: "done", repeated: false, side: "right" },
          ],
        };
        break;
      case "switch":
        newNode = {
          id, name, type, interactive: false, view,
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "default", repeated: false, side: "right" }],
        };
        break;
      default:
        newNode = {
          id, name, type, interactive: false, view,
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "out", repeated: false, side: "right" }],
        };
    }
    addNodeToStore(newNode);
  };

  return (
    <div className="relative flex-1" ref={reactFlowRef}>
      <EditToolbar
        onAddNode={handleAddNode}
        libraryEntries={libraryEntries}
        onLibraryDelete={onLibraryDelete}
        getDropPosition={computeDropPosition}
        infoOpen={infoOpen}
        onToggleInfo={onToggleInfo}
      />
      {pipeline && tab && (
        <div
          className="absolute right-3 top-2 z-10"
          data-testid="canvas-pipeline-star-container"
        >
          <PipelineStar
            tabId={tab.id}
            pipeline={pipeline}
            syncState={pipelineSync.state}
            libraryEntry={pipelineSync.entry}
            onLibraryChanged={onLibraryPipelinesChanged}
          />
        </div>
      )}
      {diagnostics.length > 0 && (
        <div className="absolute left-0 right-0 top-10 z-10">
          <LintBanner diagnostics={diagnostics} />
        </div>
      )}

      <DragHighlightProvider value={dragHighlightNodeId}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          nodeTypes={nodeTypes}
          onNodeClick={(_event, node) => {
            setSelection({ kind: "node", id: node.id });
            onCloseInfo?.();
          }}
          onEdgeClick={(_event, edge) => {
            setSelection({ kind: "node", id: edge.source });
            setScrollToPort(edge.sourceHandle ?? null);
          }}
          onPaneClick={() => setSelection({ kind: "none", id: null })}
          onConnect={onConnect}
          onConnectStart={onConnectStart}
          onConnectEnd={onConnectEnd}
          onNodeMouseEnter={onNodeMouseEnter}
          onNodeMouseLeave={onNodeMouseLeave}
          onNodeDragStop={onNodeDragStop}
          onNodeContextMenu={handleNodeContextMenu}
          onEdgeContextMenu={handleEdgeContextMenu}
          connectionLineComponent={DragConnectionLine}
          fitView
          proOptions={{ hideAttribution: true }}
          className="bg-bg-1"
          nodesDraggable
          nodesConnectable
        >
          <Background color="var(--color-line-soft)" gap={20} size={1} />
        </ReactFlow>
      </DragHighlightProvider>

      {contextMenu && (
        <ContextMenu
          {...contextMenu}
          onDeleteNode={() => {
            deleteNode(contextMenu.id);
            setContextMenu(null);
          }}
          onDuplicateNode={() => {
            duplicateNode(contextMenu.id);
            setContextMenu(null);
          }}
          onDeleteEdge={() => {
            if (contextMenu.edgeIndex !== undefined) {
              deleteEdge(contextMenu.edgeIndex);
            }
            setContextMenu(null);
          }}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}

function ContextMenu({
  x,
  y,
  type,
  onDeleteNode,
  onDuplicateNode,
  onDeleteEdge,
  onClose,
}: {
  x: number;
  y: number;
  type: "node" | "edge";
  onDeleteNode: () => void;
  onDuplicateNode: () => void;
  onDeleteEdge: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} />
      <div
        className="fixed z-50 rounded-md border border-line bg-bg-3 py-1 shadow-lg"
        style={{ left: x, top: y, fontSize: "11.5px", minWidth: 140 }}
      >
        {type === "node" ? (
          <>
            <button
              onClick={onDuplicateNode}
              className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
            >
              Duplicate
            </button>
            <button
              onClick={onDeleteNode}
              className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
            >
              Delete
            </button>
          </>
        ) : (
          <button
            onClick={onDeleteEdge}
            className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
          >
            Delete edge
          </button>
        )}
      </div>
    </>
  );
}

export default function EditCanvas(props: EditCanvasProps) {
  return (
    <ReactFlowProvider>
      <EditCanvasInner {...props} />
    </ReactFlowProvider>
  );
}
