import { useCallback, useEffect, useRef, useState } from "react";
import { Settings } from "lucide-react";
import { useDaemonSocket } from "./hooks/useDaemonSocket";
import type { ConnectionStatus } from "./hooks/useDaemonSocket";
import { useResizableLayout } from "./hooks/useResizableLayout";
import { useLibrary } from "./hooks/useLibrary";
import { useLibraryPipelines } from "./hooks/useLibraryPipelines";
import { fetchRuns, fetchRun, fetchTriggers, fetchSessions } from "./api";
import { pickLatestLiveNode } from "./lib/pickLatestLiveNode";
import { rightPaneOwner } from "./lib/rightPaneOwner";
import type { RunListEntry, RunState, NodeState, Trigger, DaemonStatus } from "./types";
import { isLiveRun } from "./types";
import SessionCounter from "./components/SessionCounter";
import ServiceHealthIndicator from "./components/ServiceHealthIndicator";
import UnifiedLeftPanel from "./components/UnifiedLeftPanel";
import NodeDetailPanel from "./components/NodeDetailPanel";
import NewRunModal from "./components/NewRunModal";
import SettingsModal from "./components/SettingsModal";
import ConflictModal from "./components/ConflictModal";
import PipelineChangedModal from "./components/PipelineChangedModal";
import SaveErrorModal from "./components/SaveErrorModal";
import { shouldPromptLibraryUpdate } from "./hooks/useLibraryPipelines";
import { useRecentReposStore } from "./stores/recentReposStore";
import type { TabId } from "./components/PipelineInfoPanel";
import EditCanvas from "./components/EditCanvas";
import TabBar from "./components/TabBar";
import NodeInspector from "./components/NodeInspector";
import MergeInspector from "./components/MergeInspector";
import PipelineInspector from "./components/PipelineInspector";
import PipelineInfoPanel from "./components/PipelineInfoPanel";
import StartInspector from "./components/StartInspector";
import EndInspector from "./components/EndInspector";
import EdgeDetailPanel from "./components/EdgeDetailPanel";
import RegionInspector from "./components/RegionInspector";
import NoteInspector from "./components/NoteInspector";
import TriggerDetailPanel from "./components/TriggerDetailPanel";
import type { TriggerPrefill } from "./components/NewRunModal";
import { deriveEdgeTrigger } from "./lib/edgeTrigger";
import { handleUndoRedoKeydown } from "./lib/undoRedoHotkeys";
import InspectorTabs from "./components/InspectorTabs";
import { useInspectorTab } from "./hooks/useInspectorTab";
import { TooltipProvider } from "./components/ui/tooltip";
import { useEditStore } from "./stores/editStore";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "./components/ui/resizable";

const PANEL_IDS = ["left", "center", "right"];
const DEFAULT_SIZES = { left: 15, center: 60, right: 25 };

const LIVE_RUN_STATUSES: ReadonlySet<string> = new Set([
  "running",
  "awaiting_user",
]);

function useRuns() {
  const [runs, setRuns] = useState<RunListEntry[]>([]);

  const refresh = useCallback(async () => {
    try {
      setRuns(await fetchRuns());
    } catch {
      // ignore
    }
  }, []);

  return { runs, refresh };
}

function useSessions() {
  const [sessions, setSessions] = useState<DaemonStatus>({ live: 0, cap: 0 });

  const refresh = useCallback(async () => {
    try {
      setSessions(await fetchSessions());
    } catch {
      // ignore
    }
  }, []);

  return { sessions, refresh };
}

function useTriggers() {
  const [triggers, setTriggers] = useState<Trigger[]>([]);

  const refresh = useCallback(async () => {
    try {
      setTriggers(await fetchTriggers());
    } catch {
      // ignore
    }
  }, []);

  return { triggers, refresh };
}

function useSelectedRun() {
  const [run, setRun] = useState<RunState | null>(null);
  const currentIdRef = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    const id = currentIdRef.current;
    if (!id) return;
    try {
      const data = await fetchRun(id);
      if (currentIdRef.current === id) setRun(data);
    } catch {
      // ignore
    }
  }, []);

  const select = useCallback(
    (newId: string | null) => {
      currentIdRef.current = newId;
      if (!newId) {
        setRun(null);
        return;
      }
      fetchRun(newId)
        .then((data) => {
          if (currentIdRef.current === newId) setRun(data);
        })
        .catch(() => {});
    },
    [],
  );

  return { run, select, refresh };
}

export default function App() {
  const { status, subscribe } = useDaemonSocket();
  const { entries: libraryEntries, refresh: refreshLibrary } = useLibrary();
  const { entries: libraryPipelines, refresh: refreshLibraryPipelines } = useLibraryPipelines();
  const { runs, refresh: refreshRuns } = useRuns();
  const { sessions, refresh: refreshSessions } = useSessions();
  const { triggers, refresh: refreshTriggers } = useTriggers();
  const [selectedTriggerId, setSelectedTriggerId] = useState<string | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const { run: selectedRun, select: selectRun, refresh: refreshRun } = useSelectedRun();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [newRunModalOpen, setNewRunModalOpen] = useState(false);
  const [settingsModalOpen, setSettingsModalOpen] = useState(false);
  // When the New Run modal is opened from a Trigger (run-now / edit), this holds
  // the source Trigger and the intended mode (#162).
  const [triggerPrefill, setTriggerPrefill] = useState<TriggerPrefill | null>(null);
  const [infoPanelOpen, setInfoPanelOpen] = useState(false);
  const [infoPanelInitialTab, setInfoPanelInitialTab] = useState<TabId | undefined>(undefined);
  const [infoPanelScrollToLine, setInfoPanelScrollToLine] = useState<number | undefined>(undefined);
  const mountedRef = useRef(false);
  const reloadPipeline = useEditStore((s) => s.reloadPipeline);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openRunPipeline = useEditStore((s) => s.openRunPipeline);
  const selection = useEditStore((s) => s.selection);
  const setSelection = useEditStore((s) => s.setSelection);
  const openTabs = useEditStore((s) => s.openTabs);
  const editSave = useEditStore((s) => s.save);
  const editUndo = useEditStore((s) => s.undo);
  const editRedo = useEditStore((s) => s.redo);
  const editActiveTabId = useEditStore((s) => s.activeTabId);
  const resolveConflict = useEditStore((s) => s.resolveConflict);
  const reloadFromLibrary = useEditStore((s) => s.reloadFromLibrary);
  const clearSaveError = useEditStore((s) => s.clearSaveError);

  // Track which library-YAML version we've already prompted about for a given
  // run-scoped tab. Re-prompting only when the library changes again avoids
  // nagging the user every time they switch back to the same run tab.
  const promptedLibraryYamlRef = useRef<Map<string, string>>(new Map());
  const [pipelineChangedPrompt, setPipelineChangedPrompt] = useState<
    { tabId: string; libraryYaml: string; pipelineName: string } | null
  >(null);

  const editTab = openTabs.find((t) => t.id === editActiveTabId);
  const editNodeType = editTab && selection.kind === "node" && selection.id
    ? editTab.pipeline.nodes.find((n) => n.id === selection.id)?.type ?? null
    : null;

  // Runtime trigger status for the selected edge (#147). Derived from the run
  // state when editing a run; the canvas never renders it.
  const selectedEdge =
    editTab && selection.kind === "edge" && selection.edgeIndex != null
      ? editTab.pipeline.edges[selection.edgeIndex] ?? null
      : null;
  const edgeTrigger =
    selectedEdge && editTab?.scope === "run"
      ? deriveEdgeTrigger(selectedRun, selectedEdge)
      : null;

  const isEditingRun = editTab?.scope === "run";
  const hasEditTab = editTab != null;

  // #315: an archived run is read-only — its worktree (and `pipeline.yaml`) is
  // gone, so any save would PUT into a 404. `isArchived` tracks the *selected*
  // run (drives the NodeDetailPanel + the archived aside below). The edit
  // affordances (Ctrl+S / undo-redo / the canvas) gate on the stricter
  // `isActiveRunArchived`: the ACTIVE tab must BE that archived run, so a
  // template tab stays editable while an archived run is merely selected.
  const isArchived = selectedRun?.status === "archived";
  const isActiveRunArchived =
    isEditingRun && editTab?.runId === selectedRun?.run_id && isArchived;

  // The Trigger backing the right-panel detail view (#162), shown whenever a
  // Trigger is selected and the info overlay is closed — even while a run-edit
  // tab owns the canvas (#247).
  //
  // A Trigger detail and a canvas selection compete for the right pane (#247).
  // The Trigger now wins over a *persistent* run-edit tab (see rightPaneOwner),
  // so we need an explicit way for the canvas to reclaim the pane — otherwise a
  // once-selected Trigger would shadow every later node/edge/region inspector.
  // Selecting a Trigger touches neither `selection` nor the active tab, so the
  // canvas-focus signal below never fires on a fresh Trigger selection; any
  // later canvas focus (a node/edge/region selection, or a tab switch/open —
  // all of which change `selection` or `editActiveTabId`) clears it. Adjusting
  // state during render (React's recommended reset-on-change pattern) rather
  // than in an effect avoids painting one stale frame of the Trigger detail.
  const [lastCanvasFocus, setLastCanvasFocus] = useState({
    selection,
    tabId: editActiveTabId,
  });
  if (
    lastCanvasFocus.selection !== selection ||
    lastCanvasFocus.tabId !== editActiveTabId
  ) {
    setLastCanvasFocus({ selection, tabId: editActiveTabId });
    if (selectedTriggerId !== null) setSelectedTriggerId(null);
  }

  const selectedTrigger =
    selectedTriggerId != null
      ? triggers.find((t) => t.id === selectedTriggerId) ?? null
      : null;

  // Which view owns the right-hand detail pane (#247). A selected Trigger now
  // wins over a persistent run-edit tab; the canvas-focus reconciliation above
  // clears `selectedTriggerId` the moment the canvas is touched again.
  const paneOwner = rightPaneOwner({
    triggerSelected: selectedTrigger != null,
    infoPanelOpen,
    hasEditTab,
  });

  const openTriggerModal = useCallback((prefill: TriggerPrefill | null) => {
    setTriggerPrefill(prefill);
    setNewRunModalOpen(true);
  }, []);

  const handleSelectTrigger = useCallback(
    (triggerId: string) => {
      // Selecting a Trigger clears the run/node selection so the detail panel
      // wins the right pane.
      setSelectedTriggerId(triggerId);
      setSelectedRunId(null);
      setSelectedNodeId(null);
      selectRun(null);
    },
    [selectRun],
  );

  const handleCloseNewRunModal = useCallback(() => {
    setNewRunModalOpen(false);
    setTriggerPrefill(null);
  }, []);

  const { activeTab: inspectorTab, setActiveTab: setInspectorTab } =
    useInspectorTab(editActiveTabId, isEditingRun);

  // The Run-pane node. A node present in the pipeline (canvas) but absent from the
  // run's node map is genuinely pending: the event-sourced projection only lists a
  // node once it has been scheduled (NodeStarted / NodeWaiting / …), so a
  // not-yet-reached downstream node has no entry. On a live run, synthesize a
  // pending NodeState so the inspector renders NodeDetailPanel (with its force-start
  // Start button, #204) instead of the passive RunTabPlaceholder — the daemon's
  // force_spawn_node already accepts a node absent from run state. Terminal runs and
  // start/end pseudo-nodes stay null.
  const runNode: NodeState | null = (() => {
    if (selection.kind !== "node" || !selection.id || !selectedRun) return null;
    const existing = selectedRun.nodes[selection.id];
    if (existing) return existing;
    if (!isLiveRun(selectedRun.status)) return null;
    const def = selectedRun.node_defs?.find((d) => d.id === selection.id);
    if (!def || def.node_type === "start" || def.node_type === "end") return null;
    return {
      node_id: selection.id,
      status: "pending",
      iter: 0,
      started_at: null,
      completed_at: null,
      failure_reason: null,
      iterations: [],
    };
  })();

  // Both inspector panes are always rendered (with the inactive one hidden
  // via the `hidden` attribute) so that switching tabs does not unmount the
  // Run pane's `<NodeDetailPanel>` — which would tear the tmux WebSocket
  // down and reattach, pushing terminal content upward on every flip.
  function inspectorRunPane() {
    if (isEditingRun && selectedRun && runNode) {
      return (
        <NodeDetailPanel
          key={runNode.node_id}
          node={runNode}
          runId={selectedRun.run_id}
          isArchived={isArchived}
          nodeName={selectedRun.node_defs?.find((d) => d.id === selection.id)?.name}
        />
      );
    }
    if (isEditingRun && selectedRun) {
      return <RunTabPlaceholder nodeId={selection.id} />;
    }
    return <NoRunPlaceholder />;
  }

  function inspectorEditPane() {
    switch (editNodeType) {
      case "merge": return <MergeInspector />;
      // #248: `script` reuses NodeInspector, which shows the Script (bash) editor
      // and hides the model field / doc-only↔code-mutating toggle for it.
      // Without this case a script node would fall through and — before the
      // in-inspector conditionals — render the wrong (agent) surface.
      case "script":
      default: return (
        <NodeInspector
          libraryEntries={libraryEntries}
          onLibraryChanged={refreshLibrary}
        />
      );
    }
  }

  const handleToggleInfo = useCallback(() => {
    setInfoPanelOpen((prev) => {
      if (!prev) {
        setInfoPanelInitialTab(undefined);
        setInfoPanelScrollToLine(undefined);
      }
      return !prev;
    });
  }, []);

  const handleCloseInfo = useCallback(() => {
    setInfoPanelOpen(false);
  }, []);

  useEffect(() => {
    if (!mountedRef.current) {
      mountedRef.current = true;
      refreshRuns();
      refreshSessions();
      refreshTriggers();
      useRecentReposStore.getState().refresh();
    }
  }, [refreshRuns, refreshSessions, refreshTriggers]);

  // A WS reconnect usually means the daemon restarted — possibly as a different
  // binary, so the /sessions payload (version included, #139) may be stale. An
  // idle daemon emits no event afterwards, so the subscribe-side refresh never
  // fires; re-fetch on every transition to "connected".
  useEffect(() => {
    if (status === "connected") {
      refreshSessions();
    }
  }, [status, refreshSessions]);

  // On a live run with nothing selected, snap selection to the latest
  // running (or awaiting_user) node so the user immediately sees its terminal.
  // Re-fires whenever the user deselects on a still-live run.
  useEffect(() => {
    if (!selectedRun) return;
    if (selection.kind === "node" && selection.id) return;
    // An explicit region/edge/note selection (#150 / #147 / #307) wins over the
    // auto-snap: the user opened an inspector and must keep it on a live run.
    if (selection.kind === "region" || selection.kind === "edge" || selection.kind === "note") return;
    if (!LIVE_RUN_STATUSES.has(selectedRun.status)) return;
    const nodeId = pickLatestLiveNode(selectedRun);
    if (!nodeId) return;
    setSelection({ kind: "node", id: nodeId });
  }, [selectedRun, selection.kind, selection.id, setSelection]);

  const handleSelectRun = useCallback(
    async (runId: string) => {
      setSelectedTriggerId(null);
      setSelectedRunId(runId);
      selectRun(runId);
      setSelectedNodeId(null);
      await openRunPipeline(runId);
    },
    [selectRun, openRunPipeline],
  );

  const handleRunCreated = useCallback(
    (runId: string) => {
      refreshRuns();
      handleSelectRun(runId);
    },
    [refreshRuns, handleSelectRun],
  );

  useEffect(() => {
    // #315: never fire a save for an archived run — the tab is read-only and a
    // PUT would 404. `isActiveRunArchived` also removes this listener the moment
    // the open run flips to archived (via refreshRun).
    if (!hasEditTab || isActiveRunArchived) return;
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (editActiveTabId) editSave(editActiveTabId);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, isActiveRunArchived, editActiveTabId, editSave]);

  // Canvas undo/redo (ADR-0014 / #226): Ctrl/Cmd+Z undo, Ctrl/Cmd+Shift+Z or
  // Ctrl/Cmd+Y redo. Sibling to the Ctrl+S effect above, but — unlike Save — it
  // MUST yield to native field undo while a text field is focused. The branch
  // logic (and that input-focus guard) lives in `handleUndoRedoKeydown` so it's
  // unit-testable without rendering the canvas; this effect just wires it up.
  useEffect(() => {
    // #315: undo/redo are edit affordances — off for a read-only archived run.
    if (!hasEditTab || isActiveRunArchived) return;
    const handler = (e: KeyboardEvent) => handleUndoRedoKeydown(e, editUndo, editRedo);
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, isActiveRunArchived, editUndo, editRedo]);

  useEffect(() => {
    return subscribe((msg) => {
      if (msg.type === "pipeline_changed" && msg.pipeline_id) {
        reloadPipeline(msg.pipeline_id);
        loadPipelines();
        return;
      }
      // Trigger lifecycle (#160/#162): create/update/delete refreshes the
      // Triggers list; a fire also creates a Run, so refresh both.
      if (
        msg.type === "trigger_created" ||
        msg.type === "trigger_updated" ||
        msg.type === "trigger_deleted" ||
        msg.type === "trigger_fired"
      ) {
        refreshTriggers();
        if (msg.type === "trigger_fired") refreshRuns();
        return;
      }
      // #315: an archived run's outputs are now preserved (ADR-0020) and its
      // `/pipeline` endpoint keeps serving, so we no longer prune the open tab
      // on `run_archived`. `refreshRun` below flips the run's status to
      // `archived`, which turns the open canvas read-only in place — the run the
      // user is watching stays put instead of vanishing.
      refreshRuns();
      refreshRun();
      // Node start/complete/fail/waiting transitions change the live session
      // count (#159) — keep the status-bar counter current.
      refreshSessions();
    });
  }, [subscribe, refreshRuns, refreshRun, refreshSessions, refreshTriggers, reloadPipeline, loadPipelines]);

  // Detect: active tab is a run whose library twin (matched by id, then name)
  // diverges from the run snapshot — pipeline or prompts, the same comparison
  // the star indicator uses. Show the modal once per (tabId, library-yaml)
  // pair so re-entering an unchanged run is silent.
  useEffect(() => {
    if (!editTab) return;
    const libEntry = shouldPromptLibraryUpdate(
      editTab,
      libraryPipelines,
      promptedLibraryYamlRef.current.get(editTab.id),
    );
    if (!libEntry) return;
    setPipelineChangedPrompt({
      tabId: editTab.id,
      libraryYaml: libEntry.yaml,
      pipelineName: editTab.pipeline.name,
    });
  }, [editTab, libraryPipelines]);

  const handlePipelineChangedKeep = useCallback(() => {
    if (!pipelineChangedPrompt) return;
    promptedLibraryYamlRef.current.set(
      pipelineChangedPrompt.tabId,
      pipelineChangedPrompt.libraryYaml,
    );
    setPipelineChangedPrompt(null);
  }, [pipelineChangedPrompt]);

  const handlePipelineChangedReload = useCallback(async () => {
    if (!pipelineChangedPrompt) return;
    promptedLibraryYamlRef.current.set(
      pipelineChangedPrompt.tabId,
      pipelineChangedPrompt.libraryYaml,
    );
    const { tabId, libraryYaml } = pipelineChangedPrompt;
    setPipelineChangedPrompt(null);
    await reloadFromLibrary(tabId, libraryYaml);
  }, [pipelineChangedPrompt, reloadFromLibrary]);

  const selectedNode =
    selectedNodeId && selectedRun
      ? selectedRun.nodes[selectedNodeId] ?? null
      : null;

  const selectedNodeType = selectedRun?.node_defs?.find(
    (d) => d.id === selectedNodeId,
  )?.node_type ?? null;

  const layout = useResizableLayout("run", PANEL_IDS, DEFAULT_SIZES);
  const minSizePx = `${layout.minSizePx}px`;
  const conflictTab = openTabs.find((t) => t.conflict != null);
  const saveErrorTab = openTabs.find((t) => t.saveError != null);

  const handleDismissSaveError = useCallback(() => {
    if (saveErrorTab) clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);

  const handleViewYaml = useCallback(() => {
    if (!saveErrorTab) return;
    setInfoPanelInitialTab("yaml");
    setInfoPanelScrollToLine(saveErrorTab.saveError?.line);
    setInfoPanelOpen(true);
    clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);

  return (
    <TooltipProvider>
    <div className="flex h-full flex-col bg-bg-1 text-fg">
      <TopBar onOpenSettings={() => setSettingsModalOpen(true)} />
      <main className="min-h-0 flex-1">
        <ResizablePanelGroup
          orientation="horizontal"
          defaultLayout={layout.defaultLayout}
          onLayoutChanged={layout.onLayoutChanged}
        >
          <ResizablePanel defaultSize={layout.defaultLayout.left} minSize={minSizePx} id="left">
            <UnifiedLeftPanel
              runs={runs}
              selectedRunId={selectedRunId}
              onSelectRun={handleSelectRun}
              onNewRun={() => setNewRunModalOpen(true)}
              libraryPipelines={libraryPipelines}
              onLibraryPipelinesChanged={refreshLibraryPipelines}
              triggers={triggers}
              selectedTriggerId={selectedTriggerId}
              onSelectTrigger={handleSelectTrigger}
              onNewTrigger={() => openTriggerModal(null)}
              onTriggersChanged={refreshTriggers}
              onRunNowTrigger={(t) => openTriggerModal({ trigger: t, mode: "run" })}
              onEditTrigger={(t) => openTriggerModal({ trigger: t, mode: "edit" })}
            />
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.center} id="center">
            {hasEditTab ? (
              <div className="flex h-full min-w-0 flex-col">
                <TabBar />
                <EditCanvas
                  libraryEntries={libraryEntries}
                  libraryPipelines={libraryPipelines}
                  onLibraryDelete={async (name) => {
                    const { deleteFromLibrary: delLib } = await import("./api");
                    await delLib(name);
                    refreshLibrary();
                  }}
                  onLibraryPipelinesChanged={refreshLibraryPipelines}
                  infoOpen={infoPanelOpen}
                  onToggleInfo={handleToggleInfo}
                  onCloseInfo={handleCloseInfo}
                  runState={selectedRun}
                />
              </div>
            ) : (
              <div className="flex h-full items-center justify-center text-fg-4" style={{ fontSize: "12px" }}>
                Select a run or open a pipeline to get started
              </div>
            )}
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.right} minSize={minSizePx} id="right" className="panel-r">
            {paneOwner === "trigger" && selectedTrigger ? (
              <TriggerDetailPanel
                key={selectedTrigger.id}
                trigger={selectedTrigger}
                onSelectRun={handleSelectRun}
              />
            ) : paneOwner === "info" ? (
              <PipelineInfoPanel
                key={infoPanelInitialTab ?? "default"}
                run={isEditingRun ? selectedRun : null}
                pipeline={editTab?.pipeline ?? null}
                libraryPipelines={libraryPipelines}
                onLibraryChanged={refreshLibraryPipelines}
                onClose={handleCloseInfo}
                initialTab={infoPanelInitialTab}
                scrollToLine={infoPanelScrollToLine}
              />
            ) : paneOwner === "editTab" ? (
              <>
                {selection.kind === "node" && editNodeType != null && editNodeType !== "start" && editNodeType !== "end" ? (
                  <InspectorTabs activeTab={inspectorTab} onTabChange={setInspectorTab}>
                    <div hidden={inspectorTab !== "run"} className="h-full" data-testid="inspector-pane-run">
                      {inspectorRunPane()}
                    </div>
                    <div hidden={inspectorTab !== "edit"} className="h-full" data-testid="inspector-pane-edit">
                      {inspectorEditPane()}
                    </div>
                  </InspectorTabs>
                ) : selection.kind === "node" && editNodeType === "start" && isEditingRun && selectedRun?.start_node && selection.id ? (
                  <StartInspector
                    startNode={selectedRun.start_node}
                    runId={selectedRun.run_id}
                    nodeId={selection.id}
                  />
                ) : selection.kind === "node" && editNodeType === "end" && isEditingRun && selectedRun?.end_node ? (
                  <EndInspector
                    endNode={selectedRun.end_node}
                  />
                ) : selection.kind === "node" ? (
                  <NodeInspector
                    libraryEntries={libraryEntries}
                    onLibraryChanged={refreshLibrary}
                  />
                ) : selection.kind === "edge" ? (
                  <EdgeDetailPanel trigger={edgeTrigger} />
                ) : selection.kind === "region" ? (
                  <RegionInspector />
                ) : selection.kind === "note" ? (
                  <NoteInspector />
                ) : null}
                {selection.kind === "none" && isEditingRun && selectedRun && (
                  <RunInfoSidebar run={selectedRun} />
                )}
                {selection.kind === "none" && !isEditingRun && (
                  <PipelineInspector
                    libraryPipelines={libraryPipelines}
                    onLibraryChanged={refreshLibraryPipelines}
                  />
                )}
              </>
            ) : (
              <>
                {selectedNodeType === "start" && selectedRun?.start_node && (
                  <StartInspector
                    startNode={selectedRun.start_node}
                    runId={selectedRun.run_id}
                    nodeId={selectedNodeId!}
                  />
                )}
                {selectedNodeType === "end" && selectedRun?.end_node && (
                  <EndInspector
                    endNode={selectedRun.end_node}
                  />
                )}
                {selectedNode && selectedRun && selectedNodeType !== "start" && selectedNodeType !== "end" && (
                  <NodeDetailPanel
                    key={selectedNode.node_id}
                    node={selectedNode}
                    runId={selectedRun.run_id}
                    isArchived={isArchived}
                    nodeName={selectedRun.node_defs?.find((d) => d.id === selectedNodeId)?.name}
                  />
                )}
                {!selectedNode && selectedNodeType !== "start" && isArchived && selectedRun && (
                  <aside className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4" style={{ fontSize: "12px" }}>
                    <div className="text-center px-6">
                      <div className="font-medium text-fg-3">Run archived</div>
                      <div className="mt-1">No live state available. Select a node to view its final status.</div>
                    </div>
                  </aside>
                )}
              </>
            )}
          </ResizablePanel>
        </ResizablePanelGroup>
      </main>
      <StatusBar status={status} sessions={sessions} />
      <NewRunModal
        open={newRunModalOpen}
        onClose={handleCloseNewRunModal}
        onCreated={(runId) => {
          handleCloseNewRunModal();
          handleRunCreated(runId);
        }}
        prefillTrigger={triggerPrefill}
        onTriggerSaved={refreshTriggers}
      />
      <SettingsModal
        open={settingsModalOpen}
        onClose={() => setSettingsModalOpen(false)}
        liveSessions={sessions.live}
        onSaved={refreshSessions}
      />
      <ConflictModal
        open={conflictTab != null}
        pipelineId={conflictTab?.id ?? ""}
        onKeep={() => {
          if (conflictTab) resolveConflict(conflictTab.id, "keep");
        }}
        onTake={() => {
          if (conflictTab) resolveConflict(conflictTab.id, "take");
        }}
      />
      <PipelineChangedModal
        open={pipelineChangedPrompt != null}
        pipelineName={pipelineChangedPrompt?.pipelineName ?? ""}
        onKeep={handlePipelineChangedKeep}
        onReload={handlePipelineChangedReload}
      />
      <SaveErrorModal
        open={saveErrorTab != null}
        error={saveErrorTab?.saveError ?? null}
        onDismiss={handleDismissSaveError}
        onViewYaml={handleViewYaml}
      />
    </div>
    </TooltipProvider>
  );
}

function TopBar({ onOpenSettings }: { onOpenSettings: () => void }) {
  return (
    <header
      className="flex h-[44px] shrink-0 items-center gap-3 border-b border-line bg-bg-2 px-3"
      style={{ fontSize: "12.5px" }}
    >
      <div className="flex items-center gap-2 border-r border-line pr-3 font-semibold tracking-tight text-fg">
        <span className="grid h-[18px] w-[18px] place-items-center text-acc">
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path
              d="M7 1L12.5 4.5V9.5L7 13L1.5 9.5V4.5L7 1Z"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinejoin="round"
            />
            <circle cx="7" cy="7" r="2" fill="currentColor" />
          </svg>
        </span>
        PDO
      </div>

      {/* Right-aligned gear → instance settings (#129). */}
      <button
        onClick={onOpenSettings}
        aria-label="Settings"
        data-testid="open-settings"
        className="ml-auto grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
      >
        <Settings size={15} />
      </button>
    </header>
  );
}

function RunInfoSidebar({ run }: { run: RunState }) {
  return (
    <aside className="flex h-full flex-col bg-bg-2" style={{ fontSize: "12px" }}>
      <div className="border-b border-line px-3 py-3">
        <div className="font-medium text-fg">{run.pipeline_name}</div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {run.run_id}
        </div>
        <div className="mt-2 rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-3" style={{ fontSize: "10.5px" }}>
          Editing run-scoped pipeline &middot; changes sync to template
        </div>
      </div>
    </aside>
  );
}

function RunTabPlaceholder({ nodeId }: { nodeId: string | null }) {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4"
      style={{ fontSize: "12px" }}
      data-testid="pending-placeholder"
    >
      <div className="px-6 text-center">
        <div className="font-medium text-fg-3">
          <em>en attente d&apos;activation</em>
        </div>
        {nodeId && (
          <div className="mt-1 font-mono" style={{ fontSize: "10px" }}>
            {nodeId}
          </div>
        )}
        <div className="mt-2 text-fg-4" style={{ fontSize: "11px" }}>
          This node is waiting for upstream dependencies to complete.
        </div>
      </div>
    </aside>
  );
}

function NoRunPlaceholder() {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4"
      style={{ fontSize: "12px" }}
    >
      <div className="px-6 text-center">
        <div className="font-medium text-fg-3">No active run</div>
        <div className="mt-1">
          Launch a run to see execution state in this tab.
        </div>
      </div>
    </aside>
  );
}

const STATUS_CONFIG: Record<ConnectionStatus, { dot: string; label: string }> = {
  connected: { dot: "bg-st-done", label: "Daemon: connected" },
  reconnecting: { dot: "bg-st-await", label: "Daemon: reconnecting…" },
  disconnected: { dot: "bg-st-failed", label: "Daemon: disconnected" },
};

function StatusBar({
  status,
  sessions,
}: {
  status: ConnectionStatus;
  sessions: DaemonStatus;
}) {
  const { dot: dotClass, label } = STATUS_CONFIG[status];

  return (
    <footer
      className="flex h-[22px] shrink-0 items-center gap-3.5 border-t border-line bg-bg-2 px-3 font-mono text-fg-3"
      style={{ fontSize: "11px" }}
    >
      <span className="flex items-center gap-1.5">
        <span className={`h-1.5 w-1.5 rounded-full ${dotClass}`} />
        {label}
      </span>
      <span className="flex-1" />
      <ServiceHealthIndicator service={sessions.service} />
      <SessionCounter live={sessions.live} cap={sessions.cap} />
      {sessions.version && <span>v{sessions.version}</span>}
    </footer>
  );
}
