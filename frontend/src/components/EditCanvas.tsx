import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ReactFlow,
  Background,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type NodeProps,
  type Connection,
  MarkerType,
  ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { NodeDef, NodeType, PipelineDef, PortBrief, PortSide } from "../types";
import type { LibraryEntry } from "../api";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";
import PortRow from "./PortRow";
import { NodeTypeIcon, CodeDocMarker } from "./NodeTypeIcon";
import { SwitchEditNode } from "./SwitchNode";
import { LoopEditNode } from "./LoopNode";
import { ForEachEditNode } from "./ForEachNode";
import { MergeEditNode } from "./MergeNode";
import EditToolbar from "./EditToolbar";
import LintBanner from "./LintBanner";
import DragConnectionLine from "./DragConnectionLine";

interface EditNodeData {
  label: string;
  nodeId: string;
  nodeType: NodeType;
  inputs: PortBrief[];
  outputs: PortBrief[];
  interactive: boolean;
  [key: string]: unknown;
}

function EditNode({ data, id }: NodeProps<Node<EditNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;
  const iconColor =
    data.nodeType === "start" ? "text-acc"
    : data.nodeType === "end" ? "text-st-blocked"
    : "text-fg-3";

  return (
    <div
      className={`rounded-md border-l-[3px] border-st-pending bg-bg-4 px-3 py-2 ${
        isSelected ? "ring-1 ring-acc" : ""
      }`}
      style={{ minWidth: 160, fontSize: "12px" }}
    >
      <div className="flex flex-col gap-0.5 mb-1">
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
      </div>
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
      <div className="mt-1 flex flex-col gap-0.5">
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
      </div>
    </div>
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

function deriveEditNodes(pipeline: PipelineDef): Node[] {
  return pipeline.nodes.map((n, i) => {
    if (n.type === "switch") {
      return {
        id: n.id,
        type: "switch",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          branches: n.outputs.map((p) => ({
            name: p.name,
            side: p.side ?? "right",
            hasWhen: p.when != null,
          })),
          inputSide: n.inputs[0]?.side ?? "left",
        },
      };
    }
    if (n.type === "merge") {
      return {
        id: n.id,
        type: "merge",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          inputSide: n.inputs[0]?.side ?? "left",
          outputSide: n.outputs[0]?.side ?? "right",
        },
      };
    }
    if (n.type === "loop") {
      return {
        id: n.id,
        type: "loop",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          maxIter: n.max_iter ?? 5,
          ports: [
            ...n.inputs.map((p) => ({ name: p.name, kind: "input" as const, side: (p.side ?? "left") as PortSide })),
            ...n.outputs.map((p) => ({ name: p.name, kind: "output" as const, side: (p.side ?? "right") as PortSide })),
          ],
        },
      };
    }
    if (n.type === "for-each") {
      return {
        id: n.id,
        type: "foreach",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          ports: [
            ...n.inputs.map((p) => ({ name: p.name, kind: "input" as const, side: (p.side ?? "left") as PortSide })),
            ...n.outputs.map((p) => ({ name: p.name, kind: "output" as const, side: (p.side ?? "right") as PortSide })),
          ],
        },
      };
    }
    return {
      id: n.id,
      type: "edit",
      position: {
        x: n.view?.x ?? 200,
        y: n.view?.y ?? 80 + i * 140,
      },
      data: {
        label: n.name ?? n.id,
        nodeId: n.id,
        nodeType: n.type,
        inputs: n.inputs.map((p) => ({ name: p.name, side: p.side ?? "left", description: p.description })),
        outputs: n.outputs.map((p) => ({ name: p.name, side: p.side ?? "right", description: p.description })),
        interactive: n.interactive,
      },
    };
  });
}

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
  onLibraryDelete: (name: string) => void;
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}

function EditCanvasInner({ libraryEntries, onLibraryDelete, infoOpen, onToggleInfo }: EditCanvasProps) {
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

  const tab = openTabs.find((t) => t.id === activeTabId);
  const pipeline = tab?.pipeline;

  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  const derivedNodes = useMemo(
    () => (pipeline ? deriveEditNodes(pipeline) : []),
    [pipeline],
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

  const handleAddNode = (type: NodeType) => {
    const id = generateNodeId();
    const name = DEFAULT_NODE_NAMES[type] ?? "node";

    const view = { x: 200, y: 80 + pipeline.nodes.length * 140 };
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
        infoOpen={infoOpen}
        onToggleInfo={onToggleInfo}
      />
      {diagnostics.length > 0 && (
        <div className="absolute left-0 right-0 top-10 z-10">
          <LintBanner diagnostics={diagnostics} />
        </div>
      )}

      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodeTypes={nodeTypes}
        onNodeClick={(_event, node) =>
          setSelection({ kind: "node", id: node.id })
        }
        onEdgeClick={(_event, edge) => {
          setSelection({ kind: "node", id: edge.source });
          setScrollToPort(edge.sourceHandle ?? null);
        }}
        onPaneClick={() => setSelection({ kind: "none", id: null })}
        onConnect={onConnect}
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
