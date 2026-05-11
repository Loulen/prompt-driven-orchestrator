import { useCallback, useEffect, useRef, useState } from "react";
import { useDaemonSocket } from "./hooks/useDaemonSocket";
import type { ConnectionStatus } from "./hooks/useDaemonSocket";
import { useResizableLayout } from "./hooks/useResizableLayout";
import { useLibrary } from "./hooks/useLibrary";
import { useLibraryPipelines } from "./hooks/useLibraryPipelines";
import { fetchRuns, fetchRun } from "./api";
import { pickLatestLiveNode } from "./lib/pickLatestLiveNode";
import type { RunListEntry, RunState } from "./types";
import UnifiedLeftPanel from "./components/UnifiedLeftPanel";
import DagCanvas from "./components/DagCanvas";
import NodeDetailPanel from "./components/NodeDetailPanel";
import NewRunModal from "./components/NewRunModal";
import ConflictModal from "./components/ConflictModal";
import PipelineChangedModal from "./components/PipelineChangedModal";
import SaveErrorModal from "./components/SaveErrorModal";
import { computePipelineSyncState } from "./hooks/useLibraryPipelines";
import { serializePipeline } from "./stores/editStore";
import type { TabId } from "./components/PipelineInfoPanel";
import EditCanvas from "./components/EditCanvas";
import TabBar from "./components/TabBar";
import NodeInspector from "./components/NodeInspector";
import SwitchInspector from "./components/SwitchInspector";
import LoopInspector from "./components/LoopInspector";
import ForEachInspector from "./components/ForEachInspector";
import MergeInspector from "./components/MergeInspector";
import PipelineInspector from "./components/PipelineInspector";
import PipelineInfoPanel from "./components/PipelineInfoPanel";
import StartInspector from "./components/StartInspector";
import EndInspector from "./components/EndInspector";
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
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const { run: selectedRun, select: selectRun, refresh: refreshRun } = useSelectedRun();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [newRunModalOpen, setNewRunModalOpen] = useState(false);
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
  const editActiveTabId = useEditStore((s) => s.activeTabId);
  const resolveConflict = useEditStore((s) => s.resolveConflict);
  const reloadFromLibrary = useEditStore((s) => s.reloadFromLibrary);
  const clearSaveError = useEditStore((s) => s.clearSaveError);
  // Tracks the node id last filled in by auto-selection. Used to decide
  // whether to start the terminal in fullsize for the current selection.
  const [autoSelectedNodeId, setAutoSelectedNodeId] = useState<string | null>(
    null,
  );

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

  const isEditingRun = editTab?.scope === "run";
  const hasEditTab = editTab != null;

  const { activeTab: inspectorTab, setActiveTab: setInspectorTab } =
    useInspectorTab(editActiveTabId, selectedRun?.status ?? null, isEditingRun);

  const runNode =
    selection.kind === "node" && selection.id && selectedRun
      ? selectedRun.nodes[selection.id] ?? null
      : null;

  function inspectorTabContent() {
    if (inspectorTab === "run") {
      if (isEditingRun && selectedRun && runNode) {
        return (
          <NodeDetailPanel
            key={runNode.node_id}
            node={runNode}
            runId={selectedRun.run_id}
            isArchived={isArchived}
            nodeName={selectedRun.node_defs?.find((d) => d.id === selection.id)?.name}
            initialTerminalExpanded={isAutoSelected}
          />
        );
      }
      if (isEditingRun && selectedRun) {
        return <RunTabPlaceholder nodeId={selection.id} />;
      }
      return <NoRunPlaceholder />;
    }

    switch (editNodeType) {
      case "switch": return <SwitchInspector />;
      case "loop": return <LoopInspector />;
      case "for-each": return <ForEachInspector />;
      case "merge": return <MergeInspector />;
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

  const handleSelectNodeInRun = useCallback((nodeId: string | null) => {
    setSelectedNodeId(nodeId);
    if (nodeId) setInfoPanelOpen(false);
  }, []);

  useEffect(() => {
    if (!mountedRef.current) {
      mountedRef.current = true;
      refreshRuns();
    }
  }, [refreshRuns]);

  // On a live run with nothing selected, snap selection to the latest
  // running (or awaiting_user) node so the user immediately sees its terminal.
  // Re-fires whenever the user deselects on a still-live run.
  useEffect(() => {
    if (!selectedRun) return;
    if (selection.kind === "node" && selection.id) return;
    if (!LIVE_RUN_STATUSES.has(selectedRun.status)) return;
    const nodeId = pickLatestLiveNode(selectedRun);
    if (!nodeId) return;
    setSelection({ kind: "node", id: nodeId });
    // eslint-disable-next-line react-hooks/set-state-in-effect -- bounded cascade: setSelection above takes the kind!="none" branch on the next run, so this effect won't re-fire and tag a second node.
    setAutoSelectedNodeId(nodeId);
  }, [selectedRun, selection.kind, selection.id, setSelection]);

  // The marker only counts while it matches the current selection; once the
  // user picks a different node manually, the comparison falls to false.
  const isAutoSelected =
    selection.kind === "node" &&
    selection.id != null &&
    selection.id === autoSelectedNodeId;

  const handleSelectRun = useCallback(
    async (runId: string) => {
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
    if (!hasEditTab) return;
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (editActiveTabId) editSave(editActiveTabId);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, editActiveTabId, editSave]);

  useEffect(() => {
    return subscribe((msg) => {
      if (msg.type === "pipeline_changed" && msg.pipeline_id) {
        reloadPipeline(msg.pipeline_id);
        loadPipelines();
      } else {
        refreshRuns();
        refreshRun();
      }
    });
  }, [subscribe, refreshRuns, refreshRun, reloadPipeline, loadPipelines]);

  // Detect: active tab is a run, library has the same pipeline name, and
  // the library YAML diverges from the run snapshot. Show the modal once per
  // (tabId, library-yaml) pair so re-entering an unchanged run is silent.
  useEffect(() => {
    if (!editTab || !editTab.runId) return;
    const libEntry = libraryPipelines.find((lp) => lp.name === editTab.pipeline.name);
    if (!libEntry) return;
    const { state } = computePipelineSyncState(
      serializePipeline(editTab.pipeline),
      [libEntry],
      editTab.pipeline.name,
    );
    if (state !== "diverged") return;
    const last = promptedLibraryYamlRef.current.get(editTab.id);
    if (last === libEntry.yaml) return;
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

  const isArchived = selectedRun?.status === "archived";

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

  const showRunDetail = !hasEditTab && selectedRun;

  return (
    <TooltipProvider>
    <div className="flex h-full flex-col bg-bg-1 text-fg">
      <TopBar />
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
            ) : showRunDetail ? (
              <div className="flex h-full min-w-0 flex-col">
                <DagCanvas
                  run={selectedRun}
                  onSelectNode={handleSelectNodeInRun}
                  selectedNodeId={selectedNodeId}
                  infoOpen={infoPanelOpen}
                  onToggleInfo={handleToggleInfo}
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
            {infoPanelOpen ? (
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
            ) : hasEditTab ? (
              <>
                {selection.kind === "node" && editNodeType != null && editNodeType !== "start" && editNodeType !== "end" ? (
                  <InspectorTabs activeTab={inspectorTab} onTabChange={setInspectorTab}>
                    {inspectorTabContent()}
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
                    initialTerminalExpanded={selectedNodeId != null && selectedNodeId === autoSelectedNodeId}
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
      <StatusBar status={status} />
      <NewRunModal
        open={newRunModalOpen}
        onClose={() => setNewRunModalOpen(false)}
        onCreated={handleRunCreated}
        libraryPipelines={libraryPipelines}
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

function TopBar() {
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
        Maestro
      </div>

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

function StatusBar({ status }: { status: ConnectionStatus }) {
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
      <span>v0.1.0</span>
    </footer>
  );
}
