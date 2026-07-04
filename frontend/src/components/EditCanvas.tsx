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
  type FinalConnectionState,
  ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { LoopKind, NodeDef, NodeStatus, NodeType, PortBrief, PortSide, RunState } from "../types";
import type { LibraryEntry, LibraryPipelineEntry } from "../api";
import { buildLoopRegionNodes, buildNoteNodes, deriveEditEdges, deriveEditNodes, edgeIndexFromId } from "./editNodeDerivation";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";
import { collectionFanoutNudges, regionsDestroyedByEdgeRemoval } from "../lib/loopRegions";
import DestroyLoopModal from "./DestroyLoopModal";
import PortRow from "./PortRow";
import { NodeTypeIcon, CodeDocMarker } from "./NodeTypeIcon";
import { NodeCard } from "./NodeCard";
import { LoopRegionNode } from "./LoopRegionNode";
import { NoteNode } from "./NoteNode";
import { MergeEditNode } from "./MergeNode";
import OrthogonalEdge from "./OrthogonalEdge";
import EditToolbar from "./EditToolbar";
import LintBanner, { type LintBannerItem } from "./LintBanner";
import DragConnectionLine from "./DragConnectionLine";
import { DragHighlightProvider, useIsDropTarget } from "./DragHighlightContext";
import PipelineStar from "./PipelineStar";
import { usePipelineLibraryState } from "../hooks/useLibraryPipelines";
import { useDismissedNudges } from "../hooks/useDismissedNudges";
import { anchorHandleId, anchorsByDropOnBody, chooseAnchorSide, isEmergentInputNode } from "../lib/anchorSide";

// The four emergent body anchor handles (#168), each pinned to its side-centre
// with the matching xyflow `Position` so a bound incoming edge arrives from that
// side. `transform` re-centres the 1px handle on the edge midpoint.
const ANCHOR_HANDLE_SIDES: { side: PortSide; position: Position; style: React.CSSProperties }[] = [
  { side: "left", position: Position.Left, style: { left: 0, top: "50%", transform: "translateY(-50%)" } },
  { side: "right", position: Position.Right, style: { right: 0, top: "50%", transform: "translateY(-50%)" } },
  { side: "top", position: Position.Top, style: { top: 0, left: "50%", transform: "translateX(-50%)" } },
  { side: "bottom", position: Position.Bottom, style: { bottom: 0, left: "50%", transform: "translateX(-50%)" } },
];

// A declared input port's side maps to the xyflow `Position` its body handle
// renders on, so a fixed-side declared port (End's `result`) arrives from its
// own declared side rather than a hardcoded left (#168 / #175 AC3).
const SIDE_TO_POSITION: Record<PortSide, Position> = {
  left: Position.Left,
  right: Position.Right,
  top: Position.Top,
  bottom: Position.Bottom,
};

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
  // Compact badge when this node is the single member of a loop region: a
  // collection (`⇉ ...`, #151) or a single-member bounded loop (`↻ ...`, #173).
  // Absent on non-member nodes and on multi-member regions (boxed instead).
  loopBadge?: { text: string; kind: LoopKind };
  [key: string]: unknown;
}

// Exported for unit tests; co-located with the canvas it renders.
export function EditNode({ data, id, selected }: NodeProps<Node<EditNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  // OR-in xyflow's own `selected` (#232) so every node in a multi-select group
  // lights the accent ring during a drag, not just the last-clicked one the
  // Zustand single-selection tracks.
  const isSelected = selected || (selection.kind === "node" && selection.id === id);
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

  // Emergent work nodes (`doc-only`/`code-mutating`) anchor incoming edges by
  // drop position; declared-port nodes (End) keep their fixed side. Keyed on
  // node TYPE so a work node carrying a vestigial declared `in` still anchors by
  // drop (#175) rather than being mistaken for a fixed-side declared port.
  const emergent = isEmergentInputNode(data.nodeType);
  // A declared port's body handle arrives from its own declared side (#175 AC3),
  // not a hardcoded left. Moot for emergent bodies (edges bind to the per-side
  // anchor handles below), but kept consistent.
  const bodyHandleSide = data.inputs[0]?.side ?? "left";

  return (
    <NodeCard status={cardStatus} selected={isSelected} style={{ minWidth: 160, fontSize: "12px" }}>
      {/* Emergent inputs (#149): NO input dots. An incoming arrow lands anywhere
          on the node body. A single invisible target handle covers the card and
          carries the drop highlight. The declared `result` input on the End node
          keeps its handle id (and its declared-side `position`) so routing to End
          still resolves on its own side; emergent nodes render the highlight
          handle id-less and bind incoming edges to the per-side anchors below. */}
      <Handle
        id={emergent ? undefined : data.inputs[0]?.name}
        type="target"
        position={SIDE_TO_POSITION[bodyHandleSide]}
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
      {/* Anchor-by-drop-position (#168): an emergent work node also renders one
          invisible side-centre target handle PER SIDE. An incoming edge binds to
          the handle for its persisted `target_side`, so xyflow anchors the arrow
          and derives the arrival geometry from that side (no forced left->right).
          Declared-input nodes (e.g. End's `result`) keep their fixed-side handle
          and never grow these. */}
      {emergent &&
        ANCHOR_HANDLE_SIDES.map(({ side, position, style }) => (
          <Handle
            key={`anchor-${side}`}
            id={anchorHandleId(side)}
            type="target"
            position={position}
            isConnectableStart={false}
            className="emergent-anchor-target"
            style={{
              position: "absolute",
              width: 1,
              height: 1,
              border: "none",
              background: "transparent",
              opacity: 0,
              ...style,
            }}
          />
        ))}
      {/* Slim card (#149): type icon + name + code/doc marker only. The node id
          and the amber interactive badge are intentionally dropped from the
          card. */}
      <div className="flex items-center gap-2">
        <NodeTypeIcon type={data.nodeType} size={14} className={`shrink-0 ${iconColor}`} />
        <span className="font-medium text-fg">{data.label}</span>
        <CodeDocMarker type={data.nodeType} />
        {data.loopBadge && (
          <span
            data-testid={data.loopBadge.kind === "collection" ? "collection-badge" : "loop-badge"}
            className="ml-auto shrink-0 rounded border border-acc px-1.5 font-mono text-acc"
            style={{ fontSize: 10, lineHeight: "16px" }}
            title={
              data.loopBadge.kind === "collection"
                ? "collection region — fans out one lap per item"
                : "bounded loop region — one member"
            }
          >
            {data.loopBadge.text}
          </span>
        )}
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

const nodeTypes = { edit: EditNode, merge: MergeEditNode, loopRegion: LoopRegionNode, note: NoteNode };
const edgeTypes = { orthogonal: OrthogonalEdge };

const DEFAULT_NODE_NAMES: Partial<Record<NodeType, string>> = {
  "code-mutating": "implementer",
  "merge": "merge",
  "script": "script",
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
  const updateNodeViews = useEditStore((s) => s.updateNodeViews);
  const addEdgeToStore = useEditStore((s) => s.addEdge);
  const deleteNode = useEditStore((s) => s.deleteNode);
  const duplicateNode = useEditStore((s) => s.duplicateNode);
  const deleteEdge = useEditStore((s) => s.deleteEdge);
  const updateEdge = useEditStore((s) => s.updateEdge);
  const addNodeToStore = useEditStore((s) => s.addNode);
  const addNoteToStore = useEditStore((s) => s.addNote);
  const moveNote = useEditStore((s) => s.moveNote);
  const deleteNote = useEditStore((s) => s.deleteNote);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    type: "node" | "edge" | "note";
    id: string;
    edgeIndex?: number;
  } | null>(null);
  // Pending destroy-loop confirmation (#150): set when a Delete-edge action would
  // remove a bounded region's last cycle. Holds the edge to delete and the loops
  // it would destroy; confirming deletes the edge (the store drops the regions).
  const [pendingDestroy, setPendingDestroy] = useState<{
    edgeIndex: number;
    loopIds: string[];
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

  // #315 / ADR-0020: an archived run's canvas is READ-ONLY. Its worktree and
  // pipeline.yaml are gone, so any edit would fire a PUT /runs/<id>/pipeline →
  // 404 → the tab self-closes and the canvas vanishes. We keep
  // selection/inspection (click a node to read its output) but disable every
  // mutation: drag, connect, add-node/add-note, and the context-menu edit
  // actions. Keyed on `archived` ONLY, so editing-during-run (ADR-0007) stays
  // intact for running/completed runs.
  const readOnly = activeRunState?.status === "archived";

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

  const derivedNodes = useMemo(() => {
    if (!pipeline) return [];
    const cards = deriveEditNodes(pipeline, activeRunState);
    // Bounded loop regions (ADR-0011 / #148) render as translucent boxes BEHIND
    // their member cards. Each multi-member region is backed by a decorative,
    // non-interactive `loopRegion` node so it tracks pan/zoom with the graph;
    // single-member regions render as a badge on the member card (no box). The
    // region nodes are prepended, pinned to a low zIndex behind the member
    // cards, and given `pointer-events: none` so edges crossing the box stay
    // clickable (#167).
    const regionNodes: Node[] = buildLoopRegionNodes(pipeline, activeRunState);
    // Inert canvas notes (#307 / ADR-0018) render as draggable/selectable cards
    // with no handle. They carry no run status, so they're derived from the
    // pipeline alone (independent of run state).
    const noteNodes: Node[] = buildNoteNodes(pipeline);
    return [...regionNodes, ...cards, ...noteNodes];
  }, [pipeline, activeRunState]);
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

  // Index the just-drawn edge will occupy, captured at `onConnect` and consumed
  // by `onConnectEnd` to stamp the drop-position anchor side (#168). The edge is
  // appended, so its index is the edge count at draw time.
  const pendingEdgeIndexRef = useRef<number | null>(null);

  const onConnect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target) return;
      if (!pipeline) return;
      const sourceNode = pipeline.nodes.find((n) => n.id === connection.source);
      const targetNode = pipeline.nodes.find((n) => n.id === connection.target);
      const sourcePort = connection.sourceHandle ?? sourceNode?.outputs[0]?.name ?? "out";
      // Inputs are emergent (#149): dropping on a node's body creates an input
      // named after the SOURCE document. Structural nodes (merge) still expose
      // declared target handles, so honour an explicit `targetHandle`; otherwise
      // the emergent name is inherited from the source.
      // The body anchor handles (#168) are LAYOUT, not semantic ports — ignore
      // them here so the emergent input name still comes from the source.
      const declaredHandle = anchorsByDropOnBody(connection.targetHandle)
        ? null
        : connection.targetHandle;
      const targetPort = declaredHandle ?? targetNode?.inputs[0]?.name ?? sourcePort;

      pendingEdgeIndexRef.current = pipeline.edges.length;
      addEdgeToStore({
        source: { node: connection.source, port: sourcePort },
        target: { node: connection.target, port: targetPort },
      });
    },
    [addEdgeToStore, pipeline],
  );

  const onNodeDragStop = useCallback(
    (_: unknown, _node: Node, nodes: Node[]) => {
      // xyflow hands us EVERY dragged node (the selected set, or just [node] for
      // a single drag) with final positions, in ONE call (#232). Partition by
      // type: pipeline nodes persist via `updateNodeViews`, notes via `moveNote`
      // (#307 trap #1). A note is NOT in `pipeline.nodes`, so `updateNodeViews`
      // would silently drop its move (unknown id → ignored) and it would snap
      // back on the next re-derivation. Decorative loop-region boxes are skipped
      // (draggable:false already, belt-and-suspenders against config drift).
      const movedNodes: { id: string; x: number; y: number }[] = [];
      for (const n of nodes) {
        if (n.type === "loopRegion") continue;
        if (n.type === "note") {
          moveNote(n.id, n.position.x, n.position.y);
          continue;
        }
        movedNodes.push({ id: n.id, x: n.position.x, y: n.position.y });
      }
      if (movedNodes.length > 0) updateNodeViews(movedNodes);
    },
    [updateNodeViews, moveNote],
  );

  const handleNodeContextMenu = useCallback(
    (event: React.MouseEvent, node: Node) => {
      event.preventDefault();
      // Loop-region boxes are decorative, not pipeline nodes — no context menu.
      if (node.type === "loopRegion") return;
      // A note (#307 trap #2) is NOT in `pipeline.nodes`, so the node lookup
      // below would miss it and its Delete would call `deleteNode(noteId)` — a
      // no-op. Give it its own `"note"` menu whose Delete routes to `deleteNote`.
      if (node.type === "note") {
        setContextMenu({
          x: event.clientX,
          y: event.clientY,
          type: "note",
          id: node.id,
        });
        return;
      }
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
  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, connectionState: FinalConnectionState) => {
      setIsDraggingEdge(false);
      setHoveredNodeId(null);

      // Anchor the just-drawn edge on the target side nearest the drop (#168).
      // Only emergent work-node bodies anchor by drop position; declared and
      // structural ports keep their fixed side. The edge `onConnect` appended is
      // at `pendingEdgeIndexRef`.
      const edgeIndex = pendingEdgeIndexRef.current;
      pendingEdgeIndexRef.current = null;
      if (edgeIndex == null) return;

      const toNode = connectionState.toNode;
      if (!toNode) return;
      const targetDef = pipeline?.nodes.find((n) => n.id === toNode.id);
      // Declared-port nodes (End's `result`) and structural nodes (merge) keep
      // their declared, fixed-side handle — never re-anchor those. Keyed on node
      // TYPE: a work node carrying a vestigial declared `in` still anchors (#175).
      if (!targetDef || !isEmergentInputNode(targetDef.type)) return;

      // Where the user actually released, in FLOW coordinates. We read the raw
      // pointer (not `connectionState.to`, which xyflow snaps to a handle centre)
      // and convert it with `screenToFlowPosition` so it shares the target rect's
      // coordinate space. The #219 bug compared `connectionState.to` (rendered px,
      // zoom/pan-scaled) against a rect built from flow units, so `chooseAnchorSide`
      // got mismatched spaces and the arrow landed on the wrong side.
      const pointer = "changedTouches" in event ? event.changedTouches[0] : event;
      if (!pointer) return;
      const drop = reactFlow.screenToFlowPosition({ x: pointer.clientX, y: pointer.clientY });

      const rect = {
        x: toNode.internals.positionAbsolute.x,
        y: toNode.internals.positionAbsolute.y,
        width: toNode.measured.width ?? 0,
        height: toNode.measured.height ?? 0,
      };
      if (rect.width === 0 || rect.height === 0) return;
      const side = chooseAnchorSide(drop, rect);
      // Left is the legacy default; only persist when the drop chose another side
      // so an ordinary left-side drop stays clean in the file.
      if (side === "left") return;
      // Untracked (ADR-0014 / #226): the preceding `addEdge` already pushed the
      // pre-edge snapshot, and this arrival-side stamp is causally linked to it
      // via `pendingEdgeIndexRef`. Folding it into that one history entry makes a
      // single edge-draw gesture undo in one step (edge + side together).
      updateEdge(edgeIndex, { target_side: side }, { track: false });
    },
    [pipeline, reactFlow, updateEdge],
  );
  const onNodeMouseEnter = useCallback((_: ReactMouseEvent, node: Node) => setHoveredNodeId(node.id), []);
  const onNodeMouseLeave = useCallback(() => setHoveredNodeId(null), []);

  // #268: advisory fan-out nudges are dismissible (persisted per pipeline);
  // correctness lint is not. `tab?.id ?? ""` keeps the hook call unconditional
  // (rules of hooks) ahead of the early return below; "" is never rendered.
  const { dismissed, dismiss } = useDismissedNudges(tab?.id ?? "");
  const allItems = useMemo<LintBannerItem[]>(() => {
    if (!tab) return [];
    const lint = (tab.diagnostics ?? []).map((m, i) => ({
      id: `lint:${i}`,
      kind: "lint" as const,
      message: m,
    }));
    const nudges = collectionFanoutNudges(tab.pipeline).map((n) => ({
      ...n,
      kind: "nudge" as const,
    }));
    return [...lint, ...nudges];
  }, [tab]);
  // Filter BEFORE the render gate so dismissing the last nudge (with no lint)
  // collapses the whole overlay. MUST depend on `dismissed` or it won't update.
  const visibleItems = useMemo(
    () => allItems.filter((it) => it.kind === "lint" || !dismissed.has(it.id)),
    [allItems, dismissed],
  );

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
      case "script":
        // #248: a script node's inputs are emergent (edge-derived), like a work
        // node — it declares none. One default output for its `output.md`.
        newNode = {
          id, name, type, interactive: false, view,
          inputs: [],
          outputs: [{ name: "out", repeated: false, side: "right" }],
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

  const handleAddNote = () => {
    // A note (#307 / ADR-0018) is created empty and selected, positioned at the
    // viewport centre — same drop logic as a node. It carries no ports/name/type.
    const id = generateNodeId();
    const view = computeDropPosition();
    addNoteToStore({ id, content: "", view });
    setSelection({ kind: "note", id: null, noteId: id });
  };

  return (
    <div className="relative flex-1" ref={reactFlowRef}>
      <EditToolbar
        onAddNode={handleAddNode}
        onAddNote={handleAddNote}
        libraryEntries={libraryEntries}
        onLibraryDelete={onLibraryDelete}
        getDropPosition={computeDropPosition}
        infoOpen={infoOpen}
        onToggleInfo={onToggleInfo}
        readOnly={readOnly}
      />
      {pipeline && tab && (
        <div
          // #225: z-20 (not z-10) so the star's popover outranks the lint-banner
          // overlay below (also z-10, painted later in DOM). The popover's own
          // z-50 is trapped inside this container's stacking context, so the bump
          // must be on the container itself, not the popover. See Part 2 of #225.
          className="absolute right-3 top-2 z-20"
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
      {/* #225: lint diagnostics are an edit-mode affordance — suppress on run tabs.
          NOTE: this also suppresses lint while editing-during-run (ADR-0007), a
          deliberate trade-off ratified at PR time. `tab` is non-null here (early
          return above). `tab.runId == null` ⇔ not a run tab (≡ tab.scope !== "run"). */}
      {visibleItems.length > 0 && tab.runId == null && (
        <div className="absolute left-0 right-0 top-10 z-10">
          <LintBanner items={visibleItems} onDismiss={dismiss} />
        </div>
      )}

      <DragHighlightProvider value={dragHighlightNodeId}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodeClick={(_event, node) => {
            // Region boxes are decorative; clicking one is a no-op (they fall
            // back to the pane selection path via their pass-through body).
            if (node.type === "loopRegion") return;
            // A note (#307) opens the NoteInspector, not the node inspector — it
            // is a canvas concept, not a pipeline node.
            if (node.type === "note") {
              setSelection({ kind: "note", id: null, noteId: node.id });
              onCloseInfo?.();
              return;
            }
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
          // #315: no context-menu edit actions (delete/duplicate) on a
          // read-only archived canvas.
          onNodeContextMenu={readOnly ? undefined : handleNodeContextMenu}
          onEdgeContextMenu={readOnly ? undefined : handleEdgeContextMenu}
          connectionLineComponent={DragConnectionLine}
          deleteKeyCode={null}
          fitView
          proOptions={{ hideAttribution: true }}
          className="bg-bg-1"
          // #315: drag + connect are off on an archived run; click-to-select stays on.
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
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
          onDeleteNote={() => {
            deleteNote(contextMenu.id);
            setContextMenu(null);
          }}
          onDuplicateNode={() => {
            duplicateNode(contextMenu.id);
            setContextMenu(null);
          }}
          onDeleteEdge={() => {
            const idx = contextMenu.edgeIndex;
            setContextMenu(null);
            if (idx === undefined) return;
            // Destroy-loop confirmation (ADR-0011 / #150): if this edge is the
            // last cycle of one or more bounded regions, confirm before deleting
            // (the store removes the destroyed `loops:` entries on confirm).
            // Deleting a non-last cycle edge proceeds immediately (no popup).
            const destroyed = pipeline
              ? regionsDestroyedByEdgeRemoval(pipeline, idx)
              : [];
            if (destroyed.length > 0) {
              setPendingDestroy({ edgeIndex: idx, loopIds: destroyed });
            } else {
              deleteEdge(idx);
            }
          }}
          onClose={() => setContextMenu(null)}
        />
      )}

      <DestroyLoopModal
        open={pendingDestroy != null}
        loopIds={pendingDestroy?.loopIds ?? []}
        onClose={() => setPendingDestroy(null)}
        onConfirm={() => {
          if (pendingDestroy) deleteEdge(pendingDestroy.edgeIndex);
          setPendingDestroy(null);
        }}
      />
    </div>
  );
}

function ContextMenu({
  x,
  y,
  type,
  onDeleteNode,
  onDeleteNote,
  onDuplicateNode,
  onDeleteEdge,
  onClose,
}: {
  x: number;
  y: number;
  type: "node" | "edge" | "note";
  onDeleteNode: () => void;
  onDeleteNote: () => void;
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
        ) : type === "note" ? (
          // #307 trap #2: a note's Delete must call `deleteNote`, not
          // `deleteNode` (which would no-op on an id absent from pipeline.nodes).
          <button
            data-testid="context-menu-delete"
            onClick={onDeleteNote}
            className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
          >
            Delete note
          </button>
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
