import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  ReactFlow,
  Background,
  Handle,
  Position,
  useNodesState,
  useEdgesState,
  useReactFlow,
  type Node,
  type Edge,
  type NodeProps,
  type Connection,
  ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { NodeDef, NodeStatus, NodeType, PortBrief, RunState } from "../types";
import type { LibraryEntry, LibraryPipelineEntry } from "../api";
import { deriveEditEdges, deriveEditNodes, edgeIndexFromId } from "./editNodeDerivation";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";
import PortRow from "./PortRow";
import { NodeTypeIcon, CodeDocMarker } from "./NodeTypeIcon";
import { NodeCard } from "./NodeCard";
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
  // True only for start/end markers on a completed run — see `markerReached`.
  reached?: boolean;
  // Filenames of images uploaded with the run's input. Only the start marker
  // surfaces these (issue #145); undefined/empty on every other node.
  inputImages?: string[];
  [key: string]: unknown;
}

// Exported for unit tests; co-located with the canvas it renders.
export function EditNode({ data, id }: NodeProps<Node<EditNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;
  const isDropTarget = useIsDropTarget(id);
  const reached = data.reached ?? false;
  // A reached start/end marker borrows the green "completed" cadre (border +
  // faint green fill) so the inline run view signals end-reached the same way
  // completed work nodes already do (issue #105). Otherwise keep the live status.
  const cardStatus: NodeStatus = reached ? "completed" : data.status;
  const iconColor =
    reached ? "text-st-done"
    : data.nodeType === "start" ? "text-acc"
    : data.nodeType === "end" ? "text-st-blocked"
    : "text-fg-3";
  // Images uploaded with the run's input ride along on the Start marker only
  // (issue #145). The canvas shows a compact, filename-tagged strip; the full
  // thumbnails live in the StartInspector.
  const inputImages =
    data.nodeType === "start" ? (data.inputImages ?? []) : [];

  return (
    <NodeCard status={cardStatus} selected={isSelected} style={{ minWidth: 160, fontSize: "12px" }}>
      {/* Emergent inputs (#149): NO input dots. An incoming arrow lands anywhere
          on the node body. A single invisible target handle covers the card so a
          drop creates/pools an input by name (inherited from the source). The
          declared `result` input on the End node keeps its handle id so routing
          to End still resolves. */}
      <Handle
        id={data.inputs.length === 1 ? data.inputs[0].name : undefined}
        type="target"
        position={Position.Left}
        isConnectableStart={false}
        className={`emergent-body-target${isDropTarget ? " is-drop" : ""}`}
        style={{
          position: "absolute",
          inset: 0,
          width: "100%",
          height: "100%",
          borderRadius: 6,
          transform: "none",
          border: "none",
          background: "transparent",
          opacity: isDropTarget ? 1 : 0,
        }}
      />
      {/* Slim card (#149): type icon + name + code/doc marker only. The node id
          and the amber interactive badge are intentionally dropped from the
          card. */}
      <div className="flex items-center gap-2">
        <NodeTypeIcon type={data.nodeType} size={14} className={`shrink-0 ${iconColor}`} />
        <span className="font-medium text-fg">{data.label}</span>
        <CodeDocMarker type={data.nodeType} />
      </div>
      {inputImages.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5" data-testid="start-node-images">
          {inputImages.map((name) => (
            <div
              key={name}
              data-testid="start-node-image-chip"
              title={name}
              className="relative h-[34px] w-[46px] overflow-hidden rounded border border-line-strong bg-bg-1"
            >
              <div
                className="absolute inset-0"
                style={{
                  backgroundImage:
                    "repeating-linear-gradient(135deg, rgba(255,255,255,0.05) 0 5px, transparent 5px 10px)",
                }}
              />
              <div
                className="absolute inset-x-0 bottom-0 truncate bg-bg-0/70 px-1 font-mono text-fg-4"
                style={{ fontSize: "7.5px" }}
              >
                {name}
              </div>
            </div>
          ))}
        </div>
      )}
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

const nodeTypes = { edit: EditNode, loop: LoopEditNode, foreach: ForEachEditNode, merge: MergeEditNode };

const DEFAULT_NODE_NAMES: Partial<Record<NodeType, string>> = {
  "code-mutating": "implementer",
  "loop": "loop",
  "for-each": "foreach",
  "merge": "merge",
};

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

  const pipelineSync = usePipelineLibraryState(
    pipeline ?? null,
    libraryPipelines,
    tab?.libraryId ?? null,
    tab?.prompts,
  );
  const setLibraryBinding = useEditStore((s) => s.setLibraryBinding);

  // Lock the library binding once we've identified a match by name. This makes
  // future renames non-destructive: even though `pipelineSync.entry` will keep
  // resolving via libraryId, the canvas-side name can drift freely until the
  // user saves, at which point the library file is updated in place.
  useEffect(() => {
    if (!tab) return;
    if (tab.libraryId) return;
    if (!pipelineSync.entry) return;
    setLibraryBinding(tab.id, pipelineSync.entry.id, pipelineSync.entry.scope);
  }, [tab, pipelineSync.entry, setLibraryBinding]);

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
      // Inputs are emergent (#149): dropping on a node's body creates an input
      // named after the SOURCE document. Structural nodes (merge/loop/for-each)
      // still expose declared target handles, so honour an explicit
      // `targetHandle`; otherwise the emergent name is inherited from the source.
      const targetPort = connection.targetHandle ?? targetNode?.inputs[0]?.name ?? sourcePort;

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
            // Clicking an edge opens the edge detail panel (#147), keyed by the
            // edge's index in pipeline.edges (decoded from its `e-{i}` id).
            const idx = edgeIndexFromId(edge.id);
            if (idx == null) return;
            setSelection({ kind: "edge", id: null, edgeIndex: idx });
            onCloseInfo?.();
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
          deleteKeyCode={null}
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
