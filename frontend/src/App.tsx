import { useCallback, useEffect, useRef, useState } from "react";
import { Pencil } from "lucide-react";
import { useDaemonSocket } from "./hooks/useDaemonSocket";
import type { ConnectionStatus } from "./hooks/useDaemonSocket";
import { fetchRuns, fetchRun } from "./api";
import type { RunListEntry, RunState } from "./types";
import RunsListPanel from "./components/RunsListPanel";
import DagCanvas from "./components/DagCanvas";
import NodeDetailPanel from "./components/NodeDetailPanel";
import PipelinesListPanel from "./components/PipelinesListPanel";
import EditCanvas from "./components/EditCanvas";
import TabBar from "./components/TabBar";
import NodeInspector from "./components/NodeInspector";
import EdgeInspector from "./components/EdgeInspector";
import PipelineInspector from "./components/PipelineInspector";
import { useEditStore } from "./stores/editStore";

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
  const { runs, refresh: refreshRuns } = useRuns();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const { run: selectedRun, select: selectRun, refresh: refreshRun } = useSelectedRun();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [editMode, setEditMode] = useState(false);
  const mountedRef = useRef(false);
  const reloadPipeline = useEditStore((s) => s.reloadPipeline);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const selection = useEditStore((s) => s.selection);

  useEffect(() => {
    if (!mountedRef.current) {
      mountedRef.current = true;
      refreshRuns();
    }
  }, [refreshRuns]);

  const handleSelectRun = useCallback(
    (runId: string) => {
      setSelectedRunId(runId);
      selectRun(runId);
      setSelectedNodeId(null);
    },
    [selectRun],
  );

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

  const isArchived = selectedRun?.status === "archived";

  return (
    <div className="flex h-full flex-col bg-bg-1 text-fg">
      <TopBar editMode={editMode} onToggleEditMode={() => setEditMode(!editMode)} />
      <main className="flex min-h-0 flex-1">
        {editMode ? (
          <>
            <PipelinesListPanel />
            <div className="flex min-w-0 flex-1 flex-col">
              <TabBar />
              <EditCanvas />
            </div>
            {selection.kind === "node" && <NodeInspector />}
            {selection.kind === "edge" && <EdgeInspector />}
            {selection.kind === "none" && <PipelineInspector />}
          </>
        ) : (
          <>
            <RunsListPanel
              runs={runs}
              selectedRunId={selectedRunId}
              onSelectRun={handleSelectRun}
            />
            <DagCanvas
              run={selectedRun}
              onSelectNode={setSelectedNodeId}
              selectedNodeId={selectedNodeId}
            />
            {selectedNode && selectedRun && (
              <NodeDetailPanel
                node={selectedNode}
                runId={selectedRun.run_id}
                isArchived={isArchived}
              />
            )}
            {!selectedNode && isArchived && selectedRun && (
              <aside className="flex w-[340px] shrink-0 flex-col items-center justify-center border-l border-line bg-bg-2 text-fg-4" style={{ fontSize: "12px" }}>
                <div className="text-center px-6">
                  <div className="font-medium text-fg-3">Run archived</div>
                  <div className="mt-1">No live state available. Select a node to view its final status.</div>
                </div>
              </aside>
            )}
          </>
        )}
      </main>
      <StatusBar status={status} />
    </div>
  );
}

function TopBar({
  editMode,
  onToggleEditMode,
}: {
  editMode: boolean;
  onToggleEditMode: () => void;
}) {
  return (
    <header
      className={`flex h-[44px] shrink-0 items-center gap-3 border-b border-line bg-bg-2 px-3 ${
        editMode ? "shadow-[inset_0_2px_0_0_var(--color-edit-tint)]" : ""
      }`}
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

      <nav className="flex min-w-0 flex-1 items-center gap-1.5 text-fg-3" style={{ fontSize: "12.5px" }}>
        <span
          className={`rounded border px-1.5 py-0.5 ${
            editMode
              ? "border-edit-tint bg-edit-tint/10 text-edit-tint"
              : "border-line-strong bg-bg-3 text-fg-2"
          }`}
          style={{ fontSize: "11px", fontWeight: 500 }}
        >
          {editMode ? "Edit" : "Run"}
        </span>
      </nav>

      <div className="ml-auto flex items-center gap-1">
        <button
          onClick={onToggleEditMode}
          className={`grid h-7 w-7 place-items-center rounded-md border transition-colors ${
            editMode
              ? "border-edit-tint bg-edit-tint/10 text-edit-tint hover:bg-edit-tint/20"
              : "border-transparent bg-transparent text-fg-3 hover:bg-bg-3 hover:text-fg"
          }`}
          title="Toggle edit mode"
        >
          <Pencil size={14} />
        </button>
      </div>
    </header>
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
