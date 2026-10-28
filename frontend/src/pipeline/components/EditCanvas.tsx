/**
 * The edit canvas: the pipeline of the active tab as an xyflow graph, its
 * toolbar, lint banner, context menu and modals.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { ReactFlow, Background, useNodesState, useEdgesState, useReactFlow, type Node, type Edge, ReactFlowProvider, ViewportPortal } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { RunState } from "../../shared/generated/RunState";
import type { LibraryEntry } from "../model/library";
import type { LibraryPipelineEntry } from "../api/libraryPipelines";
import RunShellModal from "../../session/components/RunShellModal";
import RetryAllConfirmModal from "../../run/components/runsList/RetryAllConfirmModal";
import { useCanvasNodePlay } from "../store/useCanvasNodePlay";
import { buildNoteNodes, deriveEditNodes } from "./editNodeDerivation";
import { buildLoopRegionNodes } from "./loopRegionDerivation";
import { deriveEditEdges, edgeIndexFromId } from "./editEdgeDerivation";
import { useEditStore } from "../store/editStore";
import { ViewportWiringGridOverlay } from "./WiringGridOverlay";
import { LoopRegionNode } from "./LoopRegionNode";
import { NoteNode } from "./NoteNode";
import OrthogonalEdge from "./OrthogonalEdge";
import EditToolbar from "./EditToolbar";
import ExportNodeYamlModal from "./ExportNodeYamlModal";
import AddNodeFromYamlModal from "./AddNodeFromYamlModal";
import LintBanner from "./LintBanner";
import { DraftNotices } from "./DraftNoticeBanner";
import VersionBanner from "./VersionBanner";
import RunSearchHost from "../../run/components/runSearch/RunSearchHost";
import WaitingForStartedNodesBanner, { NodePlayRetryConfirm } from "../../run/components/WaitingForStartedNodesBanner";
import DragConnectionLine from "./DragConnectionLine";
import CanvasZoomBadge from "./CanvasZoomBadge";
import { DragHighlightProvider } from "./DragHighlightContext";
import { useWiringGridStep } from "../../hooks/useWiringGrid";
import { useWiringStore } from "../../stores/wiringStore";
import { useAgentProfiles } from "../../settings/store/useAgentProfiles";
import { useSkillBank } from "../../skill/store/useSkillBank";
// #723 — orchestrator pastilles on the run-view node card.
import { childrenOfNode, countChildren, useRunChildren } from "../../run/model/orchestration";
import { useCanvasContextMenu } from "../store/useCanvasContextMenu";
import { useCanvasLint } from "../store/useCanvasLint";
import { useCanvasPlacement } from "../store/useCanvasPlacement";
import { useCanvasReveal } from "../store/useCanvasReveal";
import { CANVAS_MAX_ZOOM, CANVAS_MIN_ZOOM, usePaneToggleReframe } from "../store/usePaneToggleReframe";
import { useCanvasScale } from "../store/useCanvasScale";
import { useCanvasRunActions } from "../store/useCanvasRunActions";
import { useCanvasWiring } from "../store/useCanvasWiring";
import { useCanvasNodeDragStop } from "../store/useCanvasNodeDragStop";
import { useRunEditing } from "../store/useRunEditing";
import FinishEditingConfirmModal from "./FinishEditingConfirmModal";
import { EditNode } from "./EditNode";
import { CanvasContextMenuHost } from "./CanvasContextMenu";

const nodeTypes = { edit: EditNode, loopRegion: LoopRegionNode, note: NoteNode };
const edgeTypes = { orthogonal: OrthogonalEdge };

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
  // #949: whether App's right pane is folded; the canvas re-frames on its toggle.
  rightPaneCollapsed?: boolean;
}

function EditCanvasInner({ libraryEntries, onLibraryDelete, infoOpen, onToggleInfo, onCloseInfo, assistantActive, onOpenAssistant, runState, onSelectRun, rightPaneCollapsed = false }: EditCanvasProps) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setSelection = useEditStore((s) => s.setSelection);
  const addEdgeToStore = useEditStore((s) => s.addEdge);
  const updateEdge = useEditStore((s) => s.updateEdge);
  const addNodeToStore = useEditStore((s) => s.addNode);
  const addNoteToStore = useEditStore((s) => s.addNote);

  // #345: id of the node whose "Export as YAML…" modal is open (null = closed),
  // and whether the "Add node from YAML…" modal is open. Both are edit-mode
  // affordances gated with the rest of the context menu / toolbar on readOnly.
  const [exportNodeId, setExportNodeId] = useState<string | null>(null);
  const [addFromYamlOpen, setAddFromYamlOpen] = useState(false);
  const reactFlowRef = useRef<HTMLDivElement>(null);
  const reactFlow = useReactFlow();
  const { zoom, fitViewOptions } = useCanvasScale(reactFlow);

  // #844: the wiring grid shows only while a wire is being drawn, on the lattice
  // the gesture is currently originated on, in the feedback the reader chose.
  const setWiring = useWiringStore((s) => s.setWiring);
  const wiring = useWiringStore((s) => s.wiring);
  const gridOrigin = useWiringStore((s) => s.origin);
  const gridFeedback = useWiringStore((s) => s.gridFeedback);
  // #877 / ADR-0076: the step of the active pipeline — its own `grid_size`, else
  // the reader's global default.
  const gridStep = useWiringGridStep();

  const tab = openTabs.find((t) => t.id === activeTabId);
  const pipeline = tab?.pipeline;

  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const { contextMenu, setContextMenu, handleNodeContextMenu, handleEdgeContextMenu } =
    useCanvasContextMenu(pipeline, nodes);
  const {
    isDraggingEdge,
    hoveredNodeId,
    onConnect,
    onConnectStart,
    onConnectEnd,
    onNodeMouseEnter,
    onNodeMouseLeave,
  } = useCanvasWiring({ pipeline, reactFlow, addEdgeToStore, updateEdge, setWiring, gridStep });
  const dragHighlightNodeId = isDraggingEdge ? hoveredNodeId : null;
  useCanvasReveal(reactFlowRef, reactFlow);
  usePaneToggleReframe(reactFlowRef, reactFlow, tab != null && pipeline != null, rightPaneCollapsed);
  const { computeDropPosition, handleAddNode, handleAddNote } = useCanvasPlacement({
    pipeline,
    reactFlowRef,
    reactFlow,
    addNodeToStore,
    addNoteToStore,
    setSelection,
  });

  // Only apply live run status when the active tab is the run-scoped tab
  // matching the currently loaded RunState — otherwise (template pipelines,
  // or a different run) every node stays "pending".
  const activeRunState =
    tab?.runId && runState && tab.runId === runState.run_id ? runState : null;

  // #315 / ADR-0085: an archived Run, or a Run followed in « pilotage », is
  // read-only: selection and inspection stay, every mutation is off (drag,
  // connect, add-node/add-note, the context menu's edit actions).
  const { readOnly, runEdit, finishConfirm } = useRunEditing(tab, activeRunState);
  const {
    reviewHref,
    reviewPending,
    reviewTone,
    reviewTitle,
    finishedRun,
    runShell,
    confirmRetryAll,
    setConfirmRetryAll,
    handleReopen,
    handleRetryAll,
    handleOpenShell,
    mayReopen,
    mayRetryAll,
  } = useCanvasRunActions(activeRunState, onSelectRun);

  const { profiles: agentProfiles } = useAgentProfiles();
  const agentProfileIds = useMemo(
    () => new Set(agentProfiles.map((profile) => profile.id)),
    [agentProfiles],
  );
  // #669: the bank's ids, for the missing-skill lint above the canvas.
  const { bank: skillBank, loaded: skillBankLoaded } = useSkillBank();
  const skillIds = useMemo(() => new Set(skillBank.skills.map((skill) => skill.id)), [skillBank]);
  const { visibleItems, dismiss } = useCanvasLint(tab, agentProfileIds, skillIds, skillBankLoaded);
  // #723 — children of the run, polled with the view; mapped onto the cards of
  // nodes whose frozen `orchestrator` toggle is on. Template tabs never poll
  // (no run state → no children) and stay untouched.
  const runChildren = useRunChildren(activeRunState?.run_id ?? null, activeRunState != null);
  const { withPlay, nodePlay } = useCanvasNodePlay(pipeline, activeRunState);
  const derivedNodes = useMemo(() => {
    if (!pipeline) return [];
    const cards = deriveEditNodes(pipeline, activeRunState, agentProfileIds).map(withPlay).map((card) => {
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
  }, [pipeline, activeRunState, agentProfileIds, runChildren, withPlay]);
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

  const onNodeDragStop = useCanvasNodeDragStop();

  if (!tab || !pipeline) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-3">
        Select a pipeline to edit
      </div>
    );
  }

  // #345: resolve the export-modal target live, so a delete/undo that removes it
  // mid-modal drops back to null (closes the modal) rather than showing a stale node.
  const exportNode = exportNodeId
    ? pipeline.nodes.find((n) => n.id === exportNodeId) ?? null
    : null;

  return (
    <div className="relative flex-1" ref={reactFlowRef}>
      {/* ADR-0088: a version shown read only has its banner where the toolbar sits. */}
      {tab.version ? (
        <VersionBanner version={tab.version} infoOpen={infoOpen} onToggleInfo={onToggleInfo} />
      ) : (
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
          assistantAvailable={tab != null && tab.runId == null && tab.version == null}
          assistantActive={assistantActive}
          onOpenAssistant={onOpenAssistant}
          reviewHref={reviewHref}
          reviewPending={reviewPending}
          reviewTone={reviewTone}
          reviewTitle={reviewTitle}
          readOnly={readOnly}
          finishedRun={finishedRun}
          onReopen={mayReopen ? handleReopen : undefined}
          onRetryAll={mayRetryAll ? () => setConfirmRetryAll(true) : undefined}
          onOpenShell={handleOpenShell}
          run={activeRunState}
          runEdit={runEdit}
        />
      )}
      {/* #225: lint diagnostics are an edit-mode affordance — suppress on run tabs.
          NOTE: this also suppresses lint while editing-during-run (ADR-0007), a
          deliberate trade-off ratified at PR time. `tab` is non-null here (early
          return above). `tab.runId == null` ⇔ not a run tab (≡ tab.scope !== "run"). */}
      <WaitingForStartedNodesBanner run={activeRunState} />
      <RunSearchHost run={activeRunState} onCloseInfo={onCloseInfo} />
      {/* ADR-0088: what changed the shared draft from outside the canvas (a
          file written outside PDO, the library assistant), above the lint. */}
      {tab.runId == null && tab.version == null && (visibleItems.length > 0 || (tab.draft?.notices.length ?? 0) > 0) && (
        <div className="absolute left-0 right-0 top-10 z-10">
          <DraftNotices tab={tab} />
          {visibleItems.length > 0 && <LintBanner items={visibleItems} onDismiss={dismiss} />}
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
          // read-only canvas.
          onNodeContextMenu={readOnly ? undefined : handleNodeContextMenu}
          onEdgeContextMenu={readOnly ? undefined : handleEdgeContextMenu}
          connectionLineComponent={DragConnectionLine}
          deleteKeyCode={null}
          fitView
          // CORE-88: down to 25 %, where the rim and the snap still hold; the
          // bounds carry the UI scale (useCanvasScale).
          minZoom={CANVAS_MIN_ZOOM * zoom}
          maxZoom={CANVAS_MAX_ZOOM * zoom}
          // #825: never magnify (see canvasFitViewOptions).
          fitViewOptions={fitViewOptions}
          proOptions={{ hideAttribution: true }}
          className="pdo-unzoom bg-surface-0"
          // #315: drag + connect are off on a read-only canvas; click-to-select stays on.
          // #936: edges are born from a DRAG only; a click on a rim selects the node.
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          connectOnClick={false}
        >
          {/* Decorative background — pure chrome, but drawn at the wiring grid's
              pitch (#877) so a 30px step does not fall out of phase with it. */}
          <Background color="var(--line-soft)" gap={gridStep} size={1} />
          <CanvasZoomBadge />
          {/* #844: the wiring grid lives in FLOW space. Drawn inside the viewport
              portal, it shares the path's coordinate system exactly, so a dot is
              always a point the trace snaps to — at any zoom, and after a Shift
              re-origin. `Background`'s pattern space is SCREEN space and drifts
              from the path as soon as the canvas is panned or zoomed. */}
          {wiring && gridFeedback !== "none" && (
            <ViewportPortal>
              <ViewportWiringGridOverlay
                origin={gridOrigin}
                step={gridStep}
                variant={gridFeedback}
              />
            </ViewportPortal>
          )}
        </ReactFlow>
      </DragHighlightProvider>

      <CanvasContextMenuHost
        contextMenu={contextMenu}
        setContextMenu={setContextMenu}
        pipeline={pipeline}
        run={activeRunState}
        onExportNode={setExportNodeId}
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
      {runShell.shell && <RunShellModal session={runShell.shell.session} onClose={runShell.close} />}
      {confirmRetryAll && <RetryAllConfirmModal onConfirm={handleRetryAll} onCancel={() => setConfirmRetryAll(false)} />}
      <NodePlayRetryConfirm nodePlay={nodePlay} />
      {finishConfirm && <FinishEditingConfirmModal {...finishConfirm} />}
    </div>
  );
}

export default function EditCanvas(props: EditCanvasProps) {
  return (
    <ReactFlowProvider>
      <EditCanvasInner {...props} />
    </ReactFlowProvider>
  );
}
