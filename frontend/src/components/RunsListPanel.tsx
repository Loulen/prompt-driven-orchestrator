import { useState } from "react";
import { ChevronDown, Plus, Star, Trash2 } from "lucide-react";
import { isLiveRun, type RunListEntry, type RunStatus } from "../types";
import { cleanupRun, deleteLibraryPipeline, forgetRun } from "../api";
import type { LibraryEntry, LibraryPipelineEntry } from "../api";
import CleanupConfirmModal from "./CleanupConfirmModal";
import ForgetRunModal from "./ForgetRunModal";

const STATUS_STYLES: Record<RunStatus, { dot: string; bg: string }> = {
  running: { dot: "bg-st-running", bg: "bg-st-running-bg" },
  awaiting_user: { dot: "bg-st-await", bg: "bg-st-await-bg" },
  completed: { dot: "bg-st-done", bg: "bg-st-done-bg" },
  failed: { dot: "bg-st-failed", bg: "bg-st-failed-bg" },
  halted: { dot: "bg-st-blocked", bg: "bg-st-blocked-bg" },
  paused: { dot: "bg-st-paused", bg: "bg-st-paused-bg" },
  archived: { dot: "bg-st-archived", bg: "bg-st-archived-bg" },
};

interface Props {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onNewRun: () => void;
  libraryPipelines: LibraryPipelineEntry[];
  libraryNodes: LibraryEntry[];
  onLibraryPipelinesChanged: () => void;
}

export default function RunsListPanel({
  runs,
  selectedRunId,
  onSelectRun,
  onNewRun,
  libraryPipelines,
  libraryNodes,
  onLibraryPipelinesChanged,
}: Props) {
  const [confirmCleanup, setConfirmCleanup] = useState<
    { runId: string; status: RunStatus } | null
  >(null);
  const [confirmForget, setConfirmForget] = useState<string | null>(null);
  const [libraryOpen, setLibraryOpen] = useState(true);

  async function handleCleanup(runId: string) {
    try {
      await cleanupRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmCleanup(null);
  }

  async function handleForget(runId: string) {
    try {
      await forgetRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmForget(null);
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Runs
        <button
          onClick={onNewRun}
          className="ml-auto flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
          style={{ fontSize: "10.5px" }}
        >
          <Plus size={10} />
          New Run
        </button>
      </div>
      <div className="flex-1 overflow-y-auto">
        {runs.length === 0 && (
          <div
            className="px-3 py-4 text-center text-fg-4"
            style={{ fontSize: "11px" }}
          >
            No runs yet
          </div>
        )}
        {runs.map((run) => {
          const isSelected = run.run_id === selectedRunId;
          // A stalled run (no node running/waiting, nothing schedulable; #180)
          // is surfaced amber and steady, overriding its still-`running`
          // canonical status — "never a silent stall."
          const dot = run.stalled
            ? "bg-st-stale"
            : (STATUS_STYLES[run.status] ?? STATUS_STYLES.running).dot;
          const isArchived = run.status === "archived";
          const canCleanup = !isArchived;

          return (
            <button
              key={run.run_id}
              onClick={() => onSelectRun(run.run_id)}
              className={`group flex w-full cursor-pointer items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
                isSelected
                  ? "bg-bg-3 text-fg"
                  : "text-fg-2 hover:bg-bg-3/50"
              } ${isArchived ? "opacity-60" : ""}`}
              style={{ fontSize: "11.5px" }}
            >
              <span
                className={`h-2 w-2 shrink-0 rounded-full ${dot} ${
                  run.status === "running" && !run.stalled ? "animate-pulse" : ""
                }`}
              />
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{run.pipeline_name}</div>
                <div
                  className="truncate font-mono text-fg-4"
                  style={{ fontSize: "10px" }}
                >
                  {run.run_id.slice(0, 20)}
                </div>
              </div>
              {canCleanup && (
                <span
                  role="button"
                  title={
                    isLiveRun(run.status)
                      ? "Stop and archive run"
                      : "Cleanup run"
                  }
                  className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmCleanup({ runId: run.run_id, status: run.status });
                  }}
                >
                  <Trash2 size={12} />
                </span>
              )}
              {isArchived && (
                <span
                  role="button"
                  title="Forget this run permanently (event log + metadata)"
                  className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-st-failed group-hover:inline-flex"
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmForget(run.run_id);
                  }}
                >
                  <Trash2 size={12} />
                </span>
              )}
            </button>
          );
        })}
      </div>

      {/* Library section */}
      <div className="border-t border-line">
        <button
          type="button"
          className="flex w-full items-center gap-1.5 px-3 py-2 text-left font-medium text-fg-2 transition-colors hover:bg-bg-3/50"
          style={{ fontSize: "11.5px" }}
          onClick={() => setLibraryOpen(!libraryOpen)}
        >
          <ChevronDown
            size={12}
            className={`text-fg-3 transition-transform ${libraryOpen ? "" : "-rotate-90"}`}
          />
          Library
        </button>
        {libraryOpen && (
          <LibrarySection
            pipelines={libraryPipelines}
            nodes={libraryNodes}
            onPipelinesChanged={onLibraryPipelinesChanged}
          />
        )}
      </div>

      {confirmCleanup && (
        <CleanupConfirmModal
          isLive={
            isLiveRun(confirmCleanup.status)
          }
          onConfirm={() => handleCleanup(confirmCleanup.runId)}
          onCancel={() => setConfirmCleanup(null)}
        />
      )}

      {confirmForget && (
        <ForgetRunModal
          onConfirm={() => handleForget(confirmForget)}
          onCancel={() => setConfirmForget(null)}
        />
      )}
    </aside>
  );
}

function LibrarySection({
  pipelines,
  nodes,
  onPipelinesChanged,
}: {
  pipelines: LibraryPipelineEntry[];
  nodes: LibraryEntry[];
  onPipelinesChanged: () => void;
}) {
  return (
    <div className="pb-2" style={{ fontSize: "11px" }}>
      {/* Pipeline templates */}
      <div className="px-3 py-1 font-medium text-fg-3" style={{ fontSize: "10px", textTransform: "uppercase", letterSpacing: "0.05em" }}>
        Pipeline templates
      </div>
      {pipelines.length === 0 ? (
        <div className="px-3 py-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
          No starred templates yet
        </div>
      ) : (
        pipelines.map((p) => (
          <div
            key={p.id}
            className="group flex items-center gap-2 px-3 py-1.5 text-fg-2"
          >
            <Star size={10} className="shrink-0 fill-acc text-acc" />
            <span className="min-w-0 flex-1 truncate">{p.name}</span>
            <span className="text-fg-4" style={{ fontSize: "9px" }}>
              {p.node_count}n
            </span>
            <button
              className="hidden shrink-0 text-fg-4 transition-colors hover:text-st-failed group-hover:inline-flex"
              title="Remove from library"
              onClick={async () => {
                try {
                  await deleteLibraryPipeline(p.id);
                  onPipelinesChanged();
                } catch { /* ignore */ }
              }}
            >
              <Trash2 size={10} />
            </button>
          </div>
        ))
      )}

      {/* Reusable nodes */}
      <div className="mt-2 px-3 py-1 font-medium text-fg-3" style={{ fontSize: "10px", textTransform: "uppercase", letterSpacing: "0.05em" }}>
        Reusable nodes
      </div>
      {nodes.length === 0 ? (
        <div className="px-3 py-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
          No saved nodes yet
        </div>
      ) : (
        nodes.map((n) => (
          <div
            key={n.name}
            className="group flex items-center gap-2 px-3 py-1.5 text-fg-2"
          >
            <Star size={10} className="shrink-0 fill-acc text-acc" />
            <span className="min-w-0 flex-1 truncate">{n.name}</span>
            <span className="text-fg-4" style={{ fontSize: "9px" }}>
              {n.type}
            </span>
          </div>
        ))
      )}
    </div>
  );
}
