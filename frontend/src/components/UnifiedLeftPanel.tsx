import { useEffect, useState } from "react";
import { Plus, Trash2, ChevronDown, ChevronRight } from "lucide-react";
import type { RunListEntry, RunStatus, PipelineListEntry, PipelineScope } from "../types";
import { cleanupRun, createPipeline } from "../api";
import { useEditStore } from "../stores/editStore";
import CleanupConfirmModal from "./CleanupConfirmModal";
import ConfirmDeleteModal from "./ConfirmDeleteModal";

const STATUS_STYLES: Record<RunStatus, { dot: string }> = {
  running: { dot: "bg-st-running" },
  awaiting_user: { dot: "bg-st-await" },
  completed: { dot: "bg-st-done" },
  failed: { dot: "bg-st-failed" },
  halted: { dot: "bg-st-blocked" },
  archived: { dot: "bg-st-archived" },
};

const SCOPE_BADGE: Record<PipelineScope, { label: string; cls: string }> = {
  repo: { label: "repo", cls: "border-acc text-acc" },
  user: { label: "user", cls: "border-st-await text-st-await" },
};

interface Props {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onNewRun: () => void;
}

export default function UnifiedLeftPanel({
  runs,
  selectedRunId,
  onSelectRun,
  onNewRun,
}: Props) {
  const [confirmCleanup, setConfirmCleanup] = useState<string | null>(null);
  const [runsExpanded, setRunsExpanded] = useState(true);
  const [libraryExpanded, setLibraryExpanded] = useState(true);

  const pipelines = useEditStore((s) => s.pipelines);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const removePipeline = useEditStore((s) => s.removePipeline);
  const activeTabId = useEditStore((s) => s.activeTabId);

  const [showNewModal, setShowNewModal] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<PipelineListEntry | null>(null);

  useEffect(() => {
    loadPipelines();
  }, [loadPipelines]);

  async function handleCleanup(runId: string) {
    try {
      await cleanupRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmCleanup(null);
  }

  async function handleConfirmDelete() {
    if (!deleteTarget) return;
    try {
      await removePipeline(deleteTarget.id);
    } catch {
      // ignore
    }
    setDeleteTarget(null);
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      {/* Runs section */}
      <div
        className="flex h-[36px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        <button
          onClick={() => setRunsExpanded(!runsExpanded)}
          className="mr-1.5 flex cursor-pointer items-center text-fg-4"
        >
          {runsExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </button>
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

      {runsExpanded && (
        <div className="flex-shrink-0 overflow-y-auto" style={{ maxHeight: "50%" }}>
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
            const { dot } = STATUS_STYLES[run.status] ?? STATUS_STYLES.running;
            const isTerminal = run.status === "completed"
              || run.status === "failed"
              || run.status === "halted";

            return (
              <button
                key={run.run_id}
                onClick={() => onSelectRun(run.run_id)}
                className={`group flex w-full cursor-pointer items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
                  isSelected
                    ? "bg-bg-3 text-fg"
                    : "text-fg-2 hover:bg-bg-3/50"
                } ${run.status === "archived" ? "opacity-60" : ""}`}
                style={{ fontSize: "11.5px" }}
              >
                <span
                  className={`h-2 w-2 shrink-0 rounded-full ${dot} ${
                    run.status === "running" ? "animate-pulse" : ""
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
                {isTerminal && (
                  <span
                    role="button"
                    title="Cleanup run"
                    className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
                    onClick={(e) => {
                      e.stopPropagation();
                      setConfirmCleanup(run.run_id);
                    }}
                  >
                    <Trash2 size={12} />
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}

      {/* Library / Pipelines section */}
      <div
        className="flex h-[36px] shrink-0 items-center border-b border-t border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        <button
          onClick={() => setLibraryExpanded(!libraryExpanded)}
          className="mr-1.5 flex cursor-pointer items-center text-fg-4"
        >
          {libraryExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </button>
        Library
        <button
          onClick={() => setShowNewModal(true)}
          className="ml-auto grid h-5 w-5 cursor-pointer place-items-center rounded border border-line-strong bg-bg-3 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
          title="New pipeline"
        >
          <Plus size={12} />
        </button>
      </div>

      {libraryExpanded && (
        <div className="flex-1 overflow-y-auto">
          {pipelines.length === 0 && (
            <div
              className="px-3 py-4 text-center text-fg-4"
              style={{ fontSize: "11px" }}
            >
              No pipelines found
            </div>
          )}
          {pipelines.map((p) => {
            const badge = SCOPE_BADGE[p.scope];
            const isSelected = p.id === activeTabId;
            return (
              <button
                key={`${p.scope}-${p.id}`}
                onClick={() => openPipeline(p.id)}
                className={`group flex w-full cursor-pointer items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
                  isSelected ? "bg-bg-3 text-fg" : "text-fg-2 hover:bg-bg-3/50"
                }`}
                style={{ fontSize: "11.5px" }}
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{p.name}</div>
                  <div
                    className="mt-0.5 flex items-center gap-1.5 text-fg-4"
                    style={{ fontSize: "10px" }}
                  >
                    <span>{p.node_count} nodes</span>
                  </div>
                </div>
                <span
                  className={`shrink-0 rounded border px-1 py-px group-hover:hidden ${badge.cls}`}
                  style={{ fontSize: "9px", fontWeight: 500 }}
                >
                  {badge.label}
                </span>
                <span
                  className="hidden shrink-0 group-hover:inline-flex"
                  onClick={(e) => {
                    e.stopPropagation();
                    setDeleteTarget(p);
                  }}
                  role="button"
                  title="Delete pipeline"
                >
                  <Trash2
                    size={14}
                    className="text-fg-4 transition-colors hover:text-st-failed"
                  />
                </span>
              </button>
            );
          })}
        </div>
      )}

      {confirmCleanup && (
        <CleanupConfirmModal
          onConfirm={() => handleCleanup(confirmCleanup)}
          onCancel={() => setConfirmCleanup(null)}
        />
      )}

      <ConfirmDeleteModal
        open={deleteTarget !== null}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleConfirmDelete}
        name={deleteTarget?.name ?? ""}
      />

      {showNewModal && (
        <NewPipelineModal onClose={() => setShowNewModal(false)} />
      )}
    </aside>
  );
}

function NewPipelineModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [scope, setScope] = useState<PipelineScope>("repo");
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);

  async function handleCreate() {
    if (!name.trim()) return;
    try {
      const result = await createPipeline(name.trim(), scope);
      await loadPipelines();
      await openPipeline(result.id);
      onClose();
    } catch {
      // ignore
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        className="w-[360px] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
      >
        <div className="mb-3 font-medium text-fg">New Pipeline</div>

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Name
        </label>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-pipeline"
          className="mb-3 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none focus:border-acc"
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && handleCreate()}
        />

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Scope
        </label>
        <div className="mb-4 flex gap-1">
          {(["repo", "user"] as PipelineScope[]).map((s) => (
            <button
              key={s}
              onClick={() => setScope(s)}
              className={`rounded border px-3 py-1 font-medium transition-colors ${
                scope === s
                  ? "border-acc bg-acc-bg text-acc"
                  : "border-line-strong bg-bg-3 text-fg-3 hover:text-fg"
              }`}
              style={{ fontSize: "11px" }}
            >
              {s}
            </button>
          ))}
        </div>

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={!name.trim()}
            className="rounded bg-acc px-3 py-1 font-medium text-bg-0 transition-colors hover:bg-acc-dim disabled:opacity-50"
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
