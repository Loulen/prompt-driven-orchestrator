import { useCallback, useEffect, useRef, useState } from "react";
import { useDaemonSocket } from "./hooks/useDaemonSocket";
import type { ConnectionStatus } from "./hooks/useDaemonSocket";
import { useResizableLayout } from "./hooks/useResizableLayout";
import { useLibrary } from "./hooks/useLibrary";
import { fetchRuns, fetchRun } from "./api";
import type { RunListEntry, RunState } from "./types";
import UnifiedLeftPanel from "./components/UnifiedLeftPanel";
import DagCanvas from "./components/DagCanvas";
import NodeDetailPanel from "./components/NodeDetailPanel";
import NewRunModal from "./components/NewRunModal";
import EditCanvas from "./components/EditCanvas";
import TabBar from "./components/TabBar";
import NodeInspector from "./components/NodeInspector";
import SwitchInspector from "./components/SwitchInspector";
import LoopInspector from "./components/LoopInspector";
import PipelineInspector from "./components/PipelineInspector";
import StartInspector from "./components/StartInspector";
import EndInspector from "./components/EndInspector";
import { TooltipProvider } from "./components/ui/tooltip";
import { useEditStore } from "./stores/editStore";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "./components/ui/resizable";

const PANEL_IDS = ["left", "center", "right"];
const DEFAULT_SIZES = { left: 15, center: 60, right: 25 };

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
  const { runs, refresh: refreshRuns } = useRuns();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const { run: selectedRun, select: selectRun, refresh: refreshRun } = useSelectedRun();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [newRunModalOpen, setNewRunModalOpen] = useState(false);
  const mountedRef = useRef(false);
  const reloadPipeline = useEditStore((s) => s.reloadPipeline);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openRunPipeline = useEditStore((s) => s.openRunPipeline);
  const selection = useEditStore((s) => s.selection);
  const openTabs = useEditStore((s) => s.openTabs);
  const editSave = useEditStore((s) => s.save);
  const editActiveTabId = useEditStore((s) => s.activeTabId);

  const editTab = openTabs.find((t) => t.id === editActiveTabId);
  const editNodeType = editTab && selection.kind === "node" && selection.id
    ? editTab.pipeline.nodes.find((n) => n.id === selection.id)?.type ?? null
    : null;

  const isEditingRun = editTab?.scope === "run";
  const hasEditTab = editTab != null;

  useEffect(() => {
    if (!mountedRef.current) {
      mountedRef.current = true;
      refreshRuns();
    }
  }, [refreshRuns]);

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
            />
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.center} id="center">
            {hasEditTab ? (
              <div className="flex h-full min-w-0 flex-col">
                <TabBar />
                <EditCanvas
                  libraryEntries={libraryEntries}
                  onLibraryDelete={async (name) => {
                    const { deleteFromLibrary: delLib } = await import("./api");
                    await delLib(name);
                    refreshLibrary();
                  }}
                />
              </div>
            ) : showRunDetail ? (
              <div className="flex h-full min-w-0 flex-col">
                <DagCanvas
                  run={selectedRun}
                  onSelectNode={setSelectedNodeId}
                  selectedNodeId={selectedNodeId}
                />
              </div>
            ) : (
              <div className="flex h-full items-center justify-center text-fg-4" style={{ fontSize: "12px" }}>
                Select a run or open a pipeline to get started
              </div>
            )}
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.right} minSize={minSizePx} id="right">
            {hasEditTab ? (
              <>
                {selection.kind === "node" && editNodeType === "switch" && (
                  <SwitchInspector />
                )}
                {selection.kind === "node" && editNodeType === "loop" && (
                  <LoopInspector />
                )}
                {selection.kind === "node" && editNodeType !== "switch" && editNodeType !== "loop" && (
                  <NodeInspector
                    libraryEntries={libraryEntries}
                    onLibraryChanged={refreshLibrary}
                  />
                )}
                {selection.kind === "none" && isEditingRun && selectedRun && (
                  <RunInfoSidebar run={selectedRun} />
                )}
                {selection.kind === "none" && !isEditingRun && <PipelineInspector />}
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
      <StatusBar status={status} />
      <NewRunModal
        open={newRunModalOpen}
        onClose={() => setNewRunModalOpen(false)}
        onCreated={handleRunCreated}
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
