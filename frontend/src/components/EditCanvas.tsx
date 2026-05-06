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
  Handle,
  Position,
  MarkerType,
  ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Plus } from "lucide-react";
import type { NodeDef, NodeType, PipelineDef } from "../types";
import { useEditStore } from "../stores/editStore";
import { PREDICATE_LABELS } from "../predicates";

interface EditNodeData {
  label: string;
  nodeType: NodeType;
  inputCount: number;
  outputCount: number;
  interactive: boolean;
  [key: string]: unknown;
}

const TYPE_LABELS: Record<NodeType, string> = {
  "doc-only": "doc",
  "code-mutating": "code",
};

const TYPE_COLORS: Record<NodeType, string> = {
  "doc-only": "border-st-pending text-fg-3",
  "code-mutating": "border-acc text-acc",
};

function EditNode({ data, id }: NodeProps<Node<EditNodeData>>) {
  const typeLabel = TYPE_LABELS[data.nodeType] ?? data.nodeType;
  const typeColor = TYPE_COLORS[data.nodeType] ?? TYPE_COLORS["doc-only"];
  const selection = useEditStore((s) => s.selection);
  const isSelected = selection.kind === "node" && selection.id === id;

  return (
    <div
      className={`rounded-md border-l-[3px] border-st-pending bg-bg-4 px-3 py-2 ${
        isSelected ? "ring-1 ring-acc" : ""
      }`}
      style={{ minWidth: 160, fontSize: "12px" }}
    >
      {data.inputCount > 0 && (
        <Handle
          type="target"
          position={Position.Left}
          className="!bg-fg-4 !border-line !w-2.5 !h-2.5 hover:!bg-acc"
        />
      )}
      <div className="flex items-center gap-2">
        <span className="h-2 w-2 shrink-0 rounded-full bg-st-pending" />
        <span className="font-medium text-fg">{data.label}</span>
        {data.interactive && (
          <span
            className="rounded bg-st-await-bg px-1 font-mono text-st-await"
            style={{ fontSize: "9px" }}
          >
            interactive
          </span>
        )}
        <span
          className={`ml-auto rounded border ${typeColor} px-1 py-px`}
          style={{ fontSize: "9px", fontWeight: 500, lineHeight: "1.2" }}
        >
          {typeLabel}
        </span>
      </div>
      {data.outputCount > 0 && (
        <Handle
          type="source"
          position={Position.Right}
          className="!bg-fg-4 !border-line !w-2.5 !h-2.5 hover:!bg-acc"
        />
      )}
    </div>
  );
}

const nodeTypes = { edit: EditNode };

function formatWhenClause(when: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const [field, predicate] of Object.entries(when)) {
    if (field === "any") continue;
    if (typeof predicate === "object" && predicate !== null) {
      for (const [op, val] of Object.entries(predicate as Record<string, unknown>)) {
        const symbol = PREDICATE_LABELS[op] ?? op;
        const valStr = Array.isArray(val) ? `[${val.join(", ")}]` : String(val);
        parts.push(`${field} ${symbol} ${valStr}`);
      }
    }
  }
  return parts.join(" & ");
}

function deriveEditNodes(pipeline: PipelineDef): Node[] {
  return pipeline.nodes.map((n, i) => ({
    id: n.id,
    type: "edit",
    position: {
      x: n.view?.x ?? 200,
      y: n.view?.y ?? 80 + i * 140,
    },
    data: {
      label: n.id,
      nodeType: n.type,
      inputCount: n.inputs.length,
      outputCount: n.outputs.length,
      interactive: n.interactive,
    },
  }));
}

function deriveEditEdges(pipeline: PipelineDef): Edge[] {
  return pipeline.edges.map((e, i) => {
    const isHalt = "halt" in e.target;
    const isConditional = e.when != null;
    const isDashed = isHalt || isConditional;
    const targetId = isHalt ? `__halt__${i}` : (e.target as { node: string }).node;

    const condLabel = isConditional
      ? formatWhenClause(e.when as Record<string, unknown>)
      : undefined;

    const strokeColor = isDashed
      ? "var(--color-st-blocked, #f97316)"
      : "var(--color-fg-4)";

    return {
      id: `e-${i}`,
      source: e.source.node,
      target: targetId,
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
      label: condLabel,
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
}

function EditCanvasInner() {
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
      const sourcePort = sourceNode?.outputs[0]?.name ?? "out";
      const targetPort = targetNode?.inputs[0]?.name ?? "in";

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
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        type: "node",
        id: node.id,
      });
    },
    [],
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

  if (!tab || !pipeline) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4">
        Select a pipeline to edit
      </div>
    );
  }

  function handleAddNode(type: NodeType) {
    const existingIds = pipeline!.nodes.map((n) => n.id);
    let id = type === "code-mutating" ? "implementer" : "node";
    let counter = 1;
    while (existingIds.includes(id)) {
      id = `${type === "code-mutating" ? "implementer" : "node"}-${++counter}`;
    }

    const newNode: NodeDef = {
      id,
      type,
      prompt_file: `${activeTabId}.prompts/${id}.md`,
      inputs: [{ name: "in", repeated: false }],
      outputs: [{ name: "out", repeated: false }],
      interactive: false,
      view: { x: 200, y: 80 + pipeline!.nodes.length * 140 },
    };
    addNodeToStore(newNode);
  }

  return (
    <div className="relative flex-1" ref={reactFlowRef}>
      {/* AddPalette */}
      <div
        className="absolute left-3 top-3 z-10 flex items-center gap-1 rounded-md border border-line bg-bg-2/90 px-2 py-1.5 backdrop-blur-sm"
        style={{ fontSize: "11px" }}
      >
        <Plus size={12} className="text-fg-3" />
        <button
          onClick={() => handleAddNode("code-mutating")}
          className="rounded border border-acc bg-acc-bg px-1.5 py-0.5 font-medium text-acc transition-colors hover:bg-acc/20"
        >
          code
        </button>
        <button
          onClick={() => handleAddNode("doc-only")}
          className="rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 font-medium text-fg-3 transition-colors hover:text-fg"
        >
          doc
        </button>
      </div>

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
          const idx = parseInt(edge.id.replace("e-", ""), 10);
          setSelection({ kind: "edge", id: String(idx) });
        }}
        onPaneClick={() => setSelection({ kind: "none", id: null })}
        onConnect={onConnect}
        onNodeDragStop={onNodeDragStop}
        onNodeContextMenu={handleNodeContextMenu}
        onEdgeContextMenu={handleEdgeContextMenu}
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
              className="flex w-full items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
            >
              Duplicate
            </button>
            <button
              onClick={onDeleteNode}
              className="flex w-full items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
            >
              Delete
            </button>
          </>
        ) : (
          <button
            onClick={onDeleteEdge}
            className="flex w-full items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
          >
            Delete edge
          </button>
        )}
      </div>
    </>
  );
}

export default function EditCanvas() {
  return (
    <ReactFlowProvider>
      <EditCanvasInner />
    </ReactFlowProvider>
  );
}
