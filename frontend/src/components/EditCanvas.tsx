import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  ReactFlow,
  Background,
  Handle,
  Position,
  useNodesState,
  useEdgesState,
  useConnection,
  useReactFlow,
  type Node,
  type Edge,
  type NodeProps,
  type Connection,
  type FinalConnectionState,
  ReactFlowProvider,
  ViewportPortal,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { LoopKind, NodeDef, NodeStatus, NodeType, PortBrief, PortSide, RunState } from "../types";
import { isTerminalRun } from "../types";
import { pendingCount, pendingTone, reviewQuickAccessTitle } from "../lib/reviewComments";
import { reviewUrl } from "../lib/runRefs";
import { useReviewSeen } from "../hooks/useReviewUnread";
import type { LibraryEntry, LibraryPipelineEntry } from "../api";
import { openRunShell, reopenRun, retryAll } from "../api";
import RunShellModal from "./RunShellModal";
import { RetryAllConfirmModal } from "./UnifiedLeftPanel";
import { buildLoopRegionNodes, buildNoteNodes, deriveEditEdges, deriveEditNodes, edgeIndexFromId, resolveTargetGeometrySide } from "./editNodeDerivation";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";
import { CARD_HEIGHT, CARD_WIDTH, fallbackNodeSpot, freeDropSpot } from "../lib/nodePlacement";
import { registerCanvasReveal } from "../lib/canvasReveal";
import { collectionFanoutFields, collectionFanoutNudges, regionsDestroyedByEdgeRemoval } from "../lib/loopRegions";
import DestroyLoopModal from "./DestroyLoopModal";
import NodeRimHandles from "./NodeRimHandles";
import { ViewportWiringGridOverlay } from "./WiringGridOverlay";
import { NodeTypeIcon, IsolationMarker } from "./NodeTypeIcon";
import { NodeCard } from "./NodeCard";
import { LoopRegionNode } from "./LoopRegionNode";
import { NoteNode } from "./NoteNode";
import { MergeEditNode } from "./MergeNode";
import OrthogonalEdge from "./OrthogonalEdge";
import EditToolbar from "./EditToolbar";
import ExportNodeYamlModal from "./ExportNodeYamlModal";
import AddNodeFromYamlModal from "./AddNodeFromYamlModal";
import LintBanner, { type LintBannerItem } from "./LintBanner";
import DragConnectionLine from "./DragConnectionLine";
import { DragHighlightProvider, useIsDropTarget } from "./DragHighlightContext";
import { useDismissedNudges } from "../hooks/useDismissedNudges";
import {
  anchorFromPoint,
  anchorHandleId,
  anchorPoint,
  anchorsByDropOnBody,
  landsByDrop,
  dropAnchor,
  sideFromRimHandle,
} from "../lib/anchorSide";
import { drawnEdgeLayout } from "../lib/drawnEdge";
import { WIRING_GRID_STEP } from "../lib/wiringGrid";
import { WIRE, WIRE_SOFT } from "../lib/wiringColors";
import { pendingSource, resetWiringSession, wiringGesture, wiringTrace } from "../lib/wiringSession";
import { droppedPath } from "../lib/wiringGesture";
import { useWiringStore } from "../stores/wiringStore";
import { useAgentProfiles } from "../hooks/useAgentProfiles";
import { useSkillBank } from "../hooks/useSkillBank";
import { Bookmark, SlidersHorizontal, TriangleAlert } from "lucide-react";
// #723 — orchestrator pastilles on the run-view node card.
import { ChildCountPills } from "./OrchestrationTab";
import { childrenOfNode, countChildren, totalChildren, useRunChildren } from "../lib/orchestration";
import type { ChildCounts } from "../lib/orchestration";

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
  agentMode?: "inherit" | "profile" | "custom" | "broken";
  /** #723: the node carries the « Orchestrator » toggle (ADR-0064). */
  orchestrator?: boolean;
  /** #723: child-run counters of an orchestrator node (run view only). */
  childCounts?: ChildCounts;
  [key: string]: unknown;
}

// Exported for unit tests; co-located with the canvas it renders.
export function EditNode({ data, id, selected }: NodeProps<Node<EditNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  // #844: hovering the rim arms the gesture — a subtle AMBER ring says « a wire
  // starts here », the crosshair on the strip itself says how.
  const [rimHover, setRimHover] = useState<PortSide | null>(null);
  // #844: the drop-target ring. Read from the LIVE connection rather than from a
  // hover handler, so the card lights up exactly when releasing here would land
  // the wire — which is the same answer `onConnectEnd` will give.
  const isWireTarget = useConnection(
    (c) => c.inProgress && c.toNode?.id === id && c.fromNode?.id !== id,
  );
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

  // Work nodes and the End marker (#840) anchor incoming edges by drop
  // position, on any border. Keyed on node TYPE so a work node carrying a
  // vestigial declared `in` still anchors by drop (#175) rather than being
  // mistaken for a fixed-side declared port.
  const emergent = landsByDrop(data.nodeType);
  // A declared port's body handle arrives from its own declared side (#175 AC3),
  // not a hardcoded left. Moot for a lands-by-drop body (edges bind to the
  // per-side anchor handles below), but kept consistent.
  const bodyHandleSide = data.inputs[0]?.side ?? "left";

  return (
    <NodeCard
      status={cardStatus}
      selected={isSelected}
      style={{
        minWidth: 160,
        fontSize: "12px",
        // Amber is the wiring colour throughout (#844): the same family the
        // selected edge uses. Green is the app's accent and already means
        // something else on a card. The soft ring arms the gesture on rim hover;
        // the solid one says « releasing here lands the wire », and wins when
        // both apply.
        ...(rimHover
          ? { boxShadow: `0 0 0 1px var(--color-bg-1), 0 0 0 2.5px ${WIRE_SOFT}` }
          : null),
        ...(isWireTarget
          ? { boxShadow: `0 0 0 1px var(--color-bg-1), 0 0 0 2.5px ${WIRE}` }
          : null),
      }}
    >
      {/* Emergent inputs (#149): NO input dots. An incoming arrow lands anywhere
          on the node body. A single invisible target handle covers the card and
          carries the drop highlight. A lands-by-drop node (work nodes, End since
          #840) renders it id-less and binds incoming edges to the per-side
          anchors below; any other declared input keeps its handle id and its
          declared-side `position`. */}
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
          End grows them too (#840): its `result` is the edge's port, not a place
          on the card. */}
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
        <span className="relative shrink-0">
          <NodeTypeIcon type={data.nodeType} size={14} className={iconColor} />
          {data.agentMode === "profile" && (
            <Bookmark data-testid="agent-mode-profile" size={8} className="absolute -bottom-1 -right-1 text-fg-2" />
          )}
          {data.agentMode === "custom" && (
            <SlidersHorizontal data-testid="agent-mode-custom" size={8} className="absolute -bottom-1 -right-1 text-fg-2" />
          )}
          {data.agentMode === "broken" && (
            <TriangleAlert data-testid="agent-mode-broken" size={8} className="absolute -bottom-1 -right-1 text-st-blocked" />
          )}
        </span>
        <span className="font-medium text-fg">{data.label}</span>
        <IsolationMarker isolated={data.isolated === true} />
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
      {/* #723 — orchestrator pastilles: a row under the title (the card grows
          a line); omitted entirely without children, so a node that never
          orchestrated stays byte-identical. */}
      {data.childCounts && totalChildren(data.childCounts) > 0 && (
        <div className="mt-1.5 flex items-center gap-2 pl-[22px]" data-testid="node-child-pills-row">
          <ChildCountPills counts={data.childCounts} size="xs" testId="node-child-pills" />
        </div>
      )}
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
      {/* #844: the rim is the drag-source — no output dot any more. Rendered LAST
          and on a higher layer so it wins the pointer over the body target above;
          inside the rim the card still drags. A node that declares no output
          (the End marker) starts no wire and grows no rim. */}
      {data.outputs.length > 0 && <NodeRimHandles onRimHover={setRimHover} />}
    </NodeCard>
  );
}

const nodeTypes = { edit: EditNode, merge: MergeEditNode, loopRegion: LoopRegionNode, note: NoteNode };
const edgeTypes = { orthogonal: OrthogonalEdge };

const DEFAULT_NODE_NAMES: Partial<Record<NodeType, string>> = {
  "agent": "implementer",
  "merge": "merge",
  "script": "script",
};

interface EditCanvasProps {
  libraryEntries: LibraryEntry[];
  /** @deprecated Instance pipelines no longer have a library scope. */
  libraryPipelines?: LibraryPipelineEntry[];
  onLibraryDelete: (name: string) => void;
  /** @deprecated Instance pipelines refresh through the edit store. */
  onLibraryPipelinesChanged?: () => void;
  infoOpen?: boolean;
  onToggleInfo?: () => void;
  onCloseInfo?: () => void;
  // #302 / ADR-0048: open the info panel on the Assistant tab, and whether it is
  // the panel's current view (Bot pressed state). Wired only for template canvases.
  assistantActive?: boolean;
  onOpenAssistant?: () => void;
  runState?: RunState | null;
  // #598 / ADR-0049: navigate to another run (used after Retry-all forks a fresh
  // run). Threaded from App so the canvas toolbar's Retry-all lands on the new run.
  onSelectRun?: (runId: string) => void;
}

function EditCanvasInner({ libraryEntries, onLibraryDelete, infoOpen, onToggleInfo, onCloseInfo, assistantActive, onOpenAssistant, runState, onSelectRun }: EditCanvasProps) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setSelection = useEditStore((s) => s.setSelection);
  const updateNodeViews = useEditStore((s) => s.updateNodeViews);
  const addEdgeToStore = useEditStore((s) => s.addEdge);
  const deleteNode = useEditStore((s) => s.deleteNode);
  const duplicateNode = useEditStore((s) => s.duplicateNode);
  const createCollectionRegion = useEditStore((s) => s.createCollectionRegion);
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
    // "Fan out over a collection" gesture (#151 / #269): the member set the
    // gesture would wrap (clicked node + xyflow multi-selection, start/end
    // excluded) and the eligible `list` field names, one menu entry each.
    // Empty/absent when the selection isn't eligible.
    fanoutMembers?: string[];
    fanoutFields?: string[];
  } | null>(null);
  // Pending destroy-loop confirmation (#150): set when a Delete-edge action would
  // remove a bounded region's last cycle. Holds the edge to delete and the loops
  // it would destroy; confirming deletes the edge (the store drops the regions).
  const [pendingDestroy, setPendingDestroy] = useState<{
    edgeIndex: number;
    loopIds: string[];
  } | null>(null);
  // #345: id of the node whose "Export as YAML…" modal is open (null = closed),
  // and whether the "Add node from YAML…" modal is open. Both are edit-mode
  // affordances gated with the rest of the context menu / toolbar on readOnly.
  const [exportNodeId, setExportNodeId] = useState<string | null>(null);
  const [addFromYamlOpen, setAddFromYamlOpen] = useState(false);
  const reactFlowRef = useRef<HTMLDivElement>(null);
  const reactFlow = useReactFlow();
  const [isDraggingEdge, setIsDraggingEdge] = useState(false);
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  // #844: the wiring grid shows only while a wire is being drawn, on the lattice
  // the gesture is currently originated on, in the feedback the reader chose.
  const setWiring = useWiringStore((s) => s.setWiring);
  const wiring = useWiringStore((s) => s.wiring);
  const gridOrigin = useWiringStore((s) => s.origin);
  const gridFeedback = useWiringStore((s) => s.gridFeedback);
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

  // #752 (CONTEXT.md « Accès rapide Review »): the toolbar's Review quick access
  // — a link to the Run's Review page with the pending-comment pill — on every
  // non-archived Run (an archived Run has no diff to review). The tone follows
  // the same "seen" marker as the Diff tab's unread pill. Repositories moved to
  // a tab of the Run panel, so the old sidebar toggle is gone.
  const reviewSeen = useReviewSeen(activeRunState ?? null);
  const reviewHref =
    activeRunState != null && activeRunState.status !== "archived"
      ? reviewUrl(activeRunState.run_id)
      : undefined;
  const reviewPending = pendingCount(activeRunState?.review_comments);
  const reviewTone = pendingTone(activeRunState?.review_comments, reviewSeen);
  const reviewTitle = reviewQuickAccessTitle(activeRunState?.review_comments, reviewSeen);

  // #598 / ADR-0049: the finished-run action group is contextual to a TERMINAL,
  // non-archived run (`isTerminalRun` INCLUDES `archived`, so exclude it — an
  // archived run has no worktree to reopen or shell into). Absent on a live run.
  const finishedRun =
    activeRunState != null &&
    isTerminalRun(activeRunState.status) &&
    activeRunState.status !== "archived";

  // Ad-hoc bash shell opened on the terminal run from the toolbar (#316/#598).
  const [shellSession, setShellSession] = useState<string | null>(null);
  // Retry-all confirm gate (destructive: archive + fresh run).
  const [confirmRetryAll, setConfirmRetryAll] = useState(false);

  const handleReopen = useCallback(async () => {
    if (!activeRunState) return;
    // Optimistic: the daemon re-projects to `running` and broadcasts events; the
    // App's SSE subscription refreshes the run state, so the group disappears on
    // its own. No modal (contrast with Retry-all) — one click, one round-trip.
    try {
      await reopenRun(activeRunState.run_id);
    } catch {
      // The daemon gate may 409 (e.g. a live run raced terminal); nothing
      // actionable here — the run state refresh will reconcile the UI.
    }
  }, [activeRunState]);

  const handleRetryAll = useCallback(async () => {
    if (!activeRunState) return;
    setConfirmRetryAll(false);
    try {
      const { run_id } = await retryAll(activeRunState.run_id);
      onSelectRun?.(run_id);
    } catch {
      // Silent — mirrors the left-panel Retry-all.
    }
  }, [activeRunState, onSelectRun]);

  const handleOpenShell = useCallback(async () => {
    if (!activeRunState) return;
    try {
      const { session } = await openRunShell(activeRunState.run_id);
      setShellSession(session);
    } catch {
      // The server gate may 409 if the worktree vanished out-of-band.
    }
  }, [activeRunState]);
  const { profiles: agentProfiles } = useAgentProfiles();
  const agentProfileIds = useMemo(
    () => new Set(agentProfiles.map((profile) => profile.id)),
    [agentProfiles],
  );
  // #669: the bank's ids, for the missing-skill lint above the canvas.
  const { bank: skillBank, loaded: skillBankLoaded } = useSkillBank();
  const skillIds = useMemo(() => new Set(skillBank.skills.map((skill) => skill.id)), [skillBank]);
  // #723 — children of the run, polled with the view; mapped onto the cards of
  // nodes whose frozen `orchestrator` toggle is on. Template tabs never poll
  // (no run state → no children) and stay untouched.
  const runChildren = useRunChildren(activeRunState?.run_id ?? null, activeRunState != null);
  const derivedNodes = useMemo(() => {
    if (!pipeline) return [];
    const cards = deriveEditNodes(pipeline, activeRunState, agentProfileIds).map((card) => {
      if (!(card.data as { orchestrator?: boolean }).orchestrator) return card;
      const runId = activeRunState?.run_id ?? null;
      if (!runId) return card;
      const nodeChildren = childrenOfNode(runChildren, runId, card.id);
      if (nodeChildren.length === 0) return card;
      return { ...card, data: { ...card.data, childCounts: countChildren(nodeChildren) } };
    });
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
  }, [pipeline, activeRunState, agentProfileIds, runChildren]);
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

  // #825 (FP iteration 2): the canvas's answer to `scrollIntoView`. A card is
  // inside a transformed viewport, so scrolling the page never reaches it — a
  // guided tour pointing at a node the reader has panned away from would light a
  // rectangle off screen and wait forever. Panning only ever happens when the
  // card is NOT already in frame: whoever asks is describing a wish, and a canvas
  // that re-centred on every ask would be yanking itself around under the mouse.
  useEffect(
    () =>
      registerCanvasReveal((nodeId) => {
        const wrapper = reactFlowRef.current;
        if (!wrapper) return;
        const rect = wrapper.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) return;
        const node = reactFlow.getInternalNode(nodeId);
        if (!node) return;
        const { x, y } = node.internals.positionAbsolute;
        const w = node.measured.width ?? CARD_WIDTH;
        const h = node.measured.height ?? CARD_HEIGHT;
        const topLeft = reactFlow.screenToFlowPosition({ x: rect.left, y: rect.top });
        const bottomRight = reactFlow.screenToFlowPosition({ x: rect.right, y: rect.bottom });
        const inFrame =
          x >= topLeft.x && y >= topLeft.y && x + w <= bottomRight.x && y + h <= bottomRight.y;
        if (inFrame) return;
        reactFlow.setCenter(x + w / 2, y + h / 2, { zoom: reactFlow.getZoom(), duration: 250 });
      }),
    [reactFlow],
  );

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
      // #844: the drag starts on a RIM strip, which is a layout handle, not a
      // port. A new edge carries the FIRST DECLARED output (ADR-0073) and the
      // choice is revised in the panel's Outputs section. A structural source
      // handle (a declared port id) still names its own port.
      const sourcePort =
        (sideFromRimHandle(connection.sourceHandle) ? null : connection.sourceHandle) ??
        sourceNode?.outputs[0]?.name ??
        "out";
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
      // "Fan out over a collection" (#151 / #269): members = the clicked node
      // plus any other xyflow-selected pipeline nodes (a right-click on one
      // node of a multi-selection fans out the whole group). Start/end are
      // structural markers, never members. The gesture is offered only when
      // some incoming edge from a non-member carries a `list` field AND no
      // member already lives in a region (a node is in at most one).
      let fanoutMembers: string[] = [];
      let fanoutFields: string[] = [];
      if (pipeline) {
        const memberIds = new Set([node.id]);
        for (const n of nodes) {
          if (n.selected) memberIds.add(n.id);
        }
        fanoutMembers = pipeline.nodes
          .filter((n) => memberIds.has(n.id) && n.type !== "start" && n.type !== "end")
          .map((n) => n.id);
        const inRegion = new Set(
          (pipeline.loops ?? []).flatMap((r) => r.members),
        );
        fanoutFields = fanoutMembers.some((m) => inRegion.has(m))
          ? []
          : collectionFanoutFields(pipeline, fanoutMembers);
      }
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        type: "node",
        id: node.id,
        fanoutMembers,
        fanoutFields,
      });
    },
    [pipeline, nodes],
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

  const onConnectStart = useCallback(
    (event: MouseEvent | TouchEvent, params: { nodeId: string | null; handleId: string | null }) => {
      setIsDraggingEdge(true);
      setWiring(true);
      // #844: the press position on the rim — the one thing xyflow does not pass
      // on later, and the whole basis of the source anchor. The connection line
      // and `onConnectEnd` read it back out of the gesture scratchpad.
      resetWiringSession();
      const side = sideFromRimHandle(params.handleId);
      const internal = params.nodeId ? reactFlow.getInternalNode(params.nodeId) : null;
      if (!side || !internal) return;
      const press = "touches" in event ? event.touches[0] : event;
      if (press?.clientX == null) return;
      const pressFlow = reactFlow.screenToFlowPosition({ x: press.clientX, y: press.clientY });
      const rect = {
        x: internal.internals.positionAbsolute.x,
        y: internal.internals.positionAbsolute.y,
        width: internal.measured?.width ?? CARD_WIDTH,
        height: internal.measured?.height ?? CARD_HEIGHT,
      };
      const anchor = anchorFromPoint(pressFlow, rect, side, WIRING_GRID_STEP);
      pendingSource.anchor = anchor;
      pendingSource.point = anchorPoint(rect, anchor, anchor.side);
    },
    [reactFlow, setWiring],
  );
  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, connectionState: FinalConnectionState) => {
      setIsDraggingEdge(false);
      setWiring(false);
      setHoveredNodeId(null);

      // Anchor the just-drawn edge on the target side nearest the drop (#168), and
      // pin the route the user actually drew (#844). The edge `onConnect` appended
      // is at `pendingEdgeIndexRef`.
      const edgeIndex = pendingEdgeIndexRef.current;
      pendingEdgeIndexRef.current = null;
      const sourceAnchor = pendingSource.anchor;
      const publishedTrace = wiringTrace.points;
      const gesture = wiringGesture.current;
      resetWiringSession();
      if (edgeIndex == null) return;

      const toNode = connectionState.toNode;
      if (!toNode) return;
      const targetDef = pipeline?.nodes.find((n) => n.id === toNode.id);

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

      // Structural nodes (merge) keep their declared, fixed-side handle — never
      // re-anchor those. Work nodes and End (#840) land where dropped. Keyed on
      // node TYPE: a work node carrying a vestigial declared `in` still anchors
      // (#175).
      const anchorsByDrop = targetDef != null && landsByDrop(targetDef.type);
      const targetAnchor = anchorsByDrop ? dropAnchor(drop, rect) : null;
      // The side the edge will actually RENDER with — the same answer
      // `deriveEditEdges` gives it on the next render. A declared-port target
      // persists no `target_side` (it has no say in one), so the route must be
      // enforced against the side its handle is DECLARED on, not against the
      // `left` that absence reads back as: enforcing on `left` saved a landing
      // into a border the wire never touches, and its phantom approach ended up
      // in the file as waypoints inside the card (#844, FP finding 2).
      const side =
        targetAnchor?.side ??
        (targetDef ? resolveTargetGeometrySide(targetDef, "left") : "left");

      // The traced waypoints ARE the route: the edge is born `mode: manual`
      // (#844). Rebuilt from THIS event's connection state, not taken from the
      // last pointer move, whose hover state lags one frame behind (see
      // `droppedPath`).
      const traced = gesture
        ? droppedPath(
            gesture,
            drop,
            toNode,
            connectionState.toHandle ?? null,
            connectionState.fromNode?.id ?? null,
            WIRING_GRID_STEP,
          )
        : publishedTrace;
      // `drawnEdgeLayout` owns which fields a drop writes and which it
      // deliberately leaves absent.
      const updates = drawnEdgeLayout({
        traced,
        sourceAnchor,
        targetAnchor,
        targetSide: side,
        anchorsByDrop,
      });
      if (Object.keys(updates).length === 0) return;
      // Untracked (ADR-0014 / #226): the preceding `addEdge` already pushed the
      // pre-edge snapshot, and this layout stamp is causally linked to it via
      // `pendingEdgeIndexRef`. Folding it into that one history entry makes a
      // single edge-draw gesture undo in one step (edge + route together).
      updateEdge(edgeIndex, updates, { track: false });
    },
    [pipeline, reactFlow, updateEdge, setWiring],
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
    const missingProfiles = tab.pipeline.nodes.flatMap((node) => {
      const choice = node.agent_choice;
      if (choice?.mode !== "profile" || agentProfileIds.has(choice.profile_id)) return [];
      return [{
        id: `agent-profile:${node.id}:${choice.profile_id}`,
        kind: "lint" as const,
        message: `Agent profile ${choice.profile_id} no longer exists. Node ${node.name ?? node.id} falls back to the next tier. Pick a profile or set Custom.`,
      }];
    });
    // #669/ADR-0062: a node selecting a skill the bank no longer has. A warning,
    // never a refusal — the node runs without it. Only once the bank has loaded,
    // or every skill would flash as missing on the first render.
    const missingSkills = !skillBankLoaded
      ? []
      : tab.pipeline.nodes.flatMap((node) =>
          (node.skills ?? [])
            .filter((skill) => !skillIds.has(skill.id))
            .map((skill) => ({
              id: `skill:${node.id}:${skill.id}`,
              kind: "lint" as const,
              message: `Skill ${skill.name || skill.id} no longer exists in the bank. Node ${node.name ?? node.id} runs without it; the pipeline still launches.`,
            })),
        );
    return [...lint, ...missingProfiles, ...missingSkills, ...nudges];
  }, [tab, agentProfileIds, skillIds, skillBankLoaded]);
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

  /**
   * What the reader can actually see, in canvas units (#825, FP iteration 2). A
   * drop search that knows only canvas units puts the second card of a fresh
   * pipeline off the right edge of the screen, because « one card to the right »
   * is 440 pixels at the zoom a two-marker canvas fit-views to.
   *
   * `null` when there is nothing to measure — jsdom, or a canvas not laid out
   * yet — and the search then walks blind rather than inside a zero-sized box.
   */
  const visibleCanvasRect = (): { x: number; y: number; width: number; height: number } | null => {
    const wrapper = reactFlowRef.current;
    if (!wrapper) return null;
    const rect = wrapper.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const topLeft = reactFlow.screenToFlowPosition({ x: rect.left, y: rect.top });
    const bottomRight = reactFlow.screenToFlowPosition({ x: rect.right, y: rect.bottom });
    const width = bottomRight.x - topLeft.x;
    const height = bottomRight.y - topLeft.y;
    if (!(width > 0) || !(height > 0)) return null;
    return { x: topLeft.x, y: topLeft.y, width, height };
  };

  const computeDropPosition = (): { x: number; y: number } => {
    const visible = visibleCanvasRect();
    let cx: number;
    let cy: number;
    if (visible) {
      cx = visible.x + visible.width / 2;
      cy = visible.y + visible.height / 2;
    } else {
      cx = 200;
      cy = 200;
    }
    // Moved aside until it clears every card already on the canvas — footprints,
    // not a few pixels (`lib/nodePlacement.ts`): a card dropped 40 pixels off
    // another one is a card on top of another one, and the edges between them
    // are then unreachable.
    // A node with no persisted position is on screen too — a fresh pipeline's
    // Start and End are written without a `view`, and the search that could not
    // see them dropped the first agent right on End.
    const occupied = (pipeline?.nodes ?? []).map((n, i) => {
      const fallback = fallbackNodeSpot(i);
      return { x: n.view?.x ?? fallback.x, y: n.view?.y ?? fallback.y };
    });
    return freeDropSpot(
      { x: Math.round(cx - CARD_WIDTH / 2), y: Math.round(cy - CARD_HEIGHT / 2) },
      occupied,
      visible,
    );
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
        // #653: a new Script shares the Run worktree — the inverse default of an
        // Agent, so lightweight runtime and artifact work stays lightweight.
        newNode = {
          id, name, type, interactive: false, view,
          isolated_worktree: false,
          inputs: [],
          outputs: [{ name: "out", repeated: false, side: "right" }],
        };
        break;
      default:
        newNode = {
          id, name, type, interactive: false, view,
          // #653/ADR-0060: a new Agent is isolated. The safe placement is the
          // default, and sharing the Run worktree is an explicit opt-out.
          isolated_worktree: true,
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

  // #345: resolve the export-modal target live, so a delete/undo that removes it
  // mid-modal drops back to null (closes the modal) rather than showing a stale node.
  const exportNode = exportNodeId
    ? pipeline.nodes.find((n) => n.id === exportNodeId) ?? null
    : null;

  return (
    <div className="relative flex-1" ref={reactFlowRef}>
      <EditToolbar
        onAddNode={handleAddNode}
        onAddNote={handleAddNote}
        onAddNodeFromYaml={() => setAddFromYamlOpen(true)}
        libraryEntries={libraryEntries}
        onLibraryDelete={onLibraryDelete}
        getDropPosition={computeDropPosition}
        infoOpen={infoOpen}
        onToggleInfo={onToggleInfo}
        // #302 / ADR-0048: the Assistant is a template-only affordance — a
        // library template tab has no `runId`. On a run canvas the Bot is absent;
        // the Manager tab is reached via `(i)` there.
        assistantAvailable={tab != null && tab.runId == null}
        assistantActive={assistantActive}
        onOpenAssistant={onOpenAssistant}
        reviewHref={reviewHref}
        reviewPending={reviewPending}
        reviewTone={reviewTone}
        reviewTitle={reviewTitle}
        readOnly={readOnly}
        finishedRun={finishedRun}
        onReopen={handleReopen}
        onRetryAll={() => setConfirmRetryAll(true)}
        onOpenShell={handleOpenShell}
      />
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
          // #825 (FP iteration 2): never magnify. A pipeline with two markers in
          // it fit-views to zoom 2 by default, which makes a card 360 pixels wide
          // and fits barely two of them across the canvas — the reader's first
          // pipeline then runs out of screen after one node. Cards at the size
          // they were drawn, and a small graph simply sits in the middle.
          fitViewOptions={{ maxZoom: 1 }}
          proOptions={{ hideAttribution: true }}
          className="bg-bg-1"
          // #315: drag + connect are off on an archived run; click-to-select stays on.
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
        >
          {/* Decorative background — 20px, always, whatever the wiring grid does
              (CONTEXT.md: the two grids are different things). */}
          <Background color="var(--color-line-soft)" gap={20} size={1} />
          {/* #844: the wiring grid lives in FLOW space. Drawn inside the viewport
              portal, it shares the path's coordinate system exactly, so a dot is
              always a point the trace snaps to — at any zoom, and after a Shift
              re-origin. `Background`'s pattern space is SCREEN space and drifts
              from the path as soon as the canvas is panned or zoomed. */}
          {wiring && gridFeedback !== "none" && (
            <ViewportPortal>
              <ViewportWiringGridOverlay
                origin={gridOrigin}
                step={WIRING_GRID_STEP}
                variant={gridFeedback}
              />
            </ViewportPortal>
          )}
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
          onExportNode={() => {
            setExportNodeId(contextMenu.id);
            setContextMenu(null);
          }}
          onFanOut={(field) => {
            if (contextMenu.fanoutMembers && contextMenu.fanoutMembers.length > 0) {
              createCollectionRegion(contextMenu.fanoutMembers, field);
            }
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

      {/* #345: Export the selected node as YAML. The node is looked up live so a
          concurrent edit/delete simply closes the modal (node → undefined). */}
      {exportNode && (
        <ExportNodeYamlModal
          node={exportNode}
          prompt={tab.prompts[exportNode.id] ?? ""}
          onClose={() => setExportNodeId(null)}
        />
      )}

      {/* #345: Add a node from pasted/uploaded YAML, placed at the viewport
          centre (same drop logic as + Add node / a library insert). */}
      {addFromYamlOpen && (
        <AddNodeFromYamlModal
          getDropPosition={computeDropPosition}
          onClose={() => setAddFromYamlOpen(false)}
        />
      )}

      {/* #598 / ADR-0049: the finished-run group's Open-shell and Retry-all
          modals, reusing the exact components the left-panel row uses. */}
      {shellSession && (
        <RunShellModal
          session={shellSession}
          onClose={() => setShellSession(null)}
        />
      )}
      {confirmRetryAll && (
        <RetryAllConfirmModal
          onConfirm={handleRetryAll}
          onCancel={() => setConfirmRetryAll(false)}
        />
      )}
    </div>
  );
}

export function ContextMenu({
  x,
  y,
  type,
  fanoutFields,
  onDeleteNode,
  onDeleteNote,
  onDuplicateNode,
  onExportNode,
  onFanOut,
  onDeleteEdge,
  onClose,
}: {
  x: number;
  y: number;
  type: "node" | "edge" | "note";
  fanoutFields?: string[];
  onDeleteNode: () => void;
  onDeleteNote: () => void;
  onDuplicateNode: () => void;
  onExportNode: () => void;
  onFanOut: (field: string) => void;
  onDeleteEdge: () => void;
  onClose: () => void;
}) {
  // Escape dismisses the menu. Without it the full-screen backdrop below stayed
  // mounted after the menu was abandoned by keyboard, and silently swallowed the
  // next click on the canvas or the edge panel (#844 FP iter-2).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <div
        className="fixed inset-0 z-40"
        data-testid="context-menu-backdrop"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
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
            {/* "Fan out over a collection" (#151 / #269): one entry per
                eligible `list` field flowing into the selection. Absent when
                nothing is eligible — the gesture stays discoverable via the
                lint-banner nudge. */}
            {(fanoutFields ?? []).map((field) => (
              <button
                key={field}
                data-testid={`ctx-fanout-${field}`}
                onClick={() => onFanOut(field)}
                className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
              >
                Fan out over "{field}"
              </button>
            ))}
            {/* #345: front-only export of this node to YAML (no daemon). */}
            <button
              data-testid="ctx-export-node"
              onClick={onExportNode}
              className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
            >
              Export as YAML…
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
