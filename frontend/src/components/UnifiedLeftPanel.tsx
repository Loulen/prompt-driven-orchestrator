import { useEffect, useRef, useState } from "react";
import { Copy, Pencil, Plus, Star, Trash2, Zap } from "lucide-react";
import { isLiveRun, type RunListEntry, type RunStatus, type PipelineListEntry, type PipelineScope, type Trigger } from "../types";
import type { LibraryPipelineEntry } from "../api";
import { cleanupRun, createPipeline, deleteLibraryPipeline, duplicateLibraryPipeline, forgetRun, renameRun } from "../api";
import { useEditStore } from "../stores/editStore";
import CleanupConfirmModal from "./CleanupConfirmModal";
import ConfirmDeleteModal from "./ConfirmDeleteModal";
import ForgetRunModal from "./ForgetRunModal";
import TriggersListPanel from "./TriggersListPanel";

type LeftTab = "runs" | "triggers" | "library";

const STATUS_STYLES: Record<RunStatus, { dot: string }> = {
  running: { dot: "bg-st-running" },
  awaiting_user: { dot: "bg-st-await" },
  completed: { dot: "bg-st-done" },
  failed: { dot: "bg-st-failed" },
  skipped: { dot: "bg-st-skipped" },
  halted: { dot: "bg-st-blocked" },
  paused: { dot: "bg-st-paused" },
  archived: { dot: "bg-st-archived" },
};

const SCOPE_BADGE: Record<PipelineScope, { label: string; cls: string }> = {
  repo: { label: "repo", cls: "border-acc text-acc" },
  user: { label: "user", cls: "border-st-await text-st-await" },
  library: { label: "library", cls: "border-st-await text-st-await" },
};

interface Props {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onNewRun: () => void;
  libraryPipelines: LibraryPipelineEntry[];
  onLibraryPipelinesChanged: () => void;
  /** Triggers (#160). Optional so existing callers/tests keep working. */
  triggers?: Trigger[];
  selectedTriggerId?: string | null;
  onSelectTrigger?: (triggerId: string) => void;
  onNewTrigger?: () => void;
  onTriggersChanged?: () => void;
  /** Run-now / edit a Trigger via the New Run modal (#162). */
  onRunNowTrigger?: (trigger: Trigger) => void;
  onEditTrigger?: (trigger: Trigger) => void;
}

export default function UnifiedLeftPanel({
  runs,
  selectedRunId,
  onSelectRun,
  onNewRun,
  libraryPipelines,
  onLibraryPipelinesChanged,
  triggers = [],
  selectedTriggerId = null,
  onSelectTrigger,
  onNewTrigger,
  onTriggersChanged,
  onRunNowTrigger,
  onEditTrigger,
}: Props) {
  const [activeTab, setActiveTab] = useState<LeftTab>("runs");
  const [confirmCleanup, setConfirmCleanup] = useState<
    { runId: string; status: RunStatus } | null
  >(null);
  const [confirmForget, setConfirmForget] = useState<string | null>(null);
  const [renamingRunId, setRenamingRunId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  const pipelines = useEditStore((s) => s.pipelines);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const removePipeline = useEditStore((s) => s.removePipeline);
  const activeTabId = useEditStore((s) => s.activeTabId);

  const [showNewModal, setShowNewModal] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<PipelineListEntry | null>(null);
  // Busy guard so a double-click on a library row's Copy icon fires once (#224).
  const [duplicatingId, setDuplicatingId] = useState<string | null>(null);

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

  async function handleForget(runId: string) {
    try {
      await forgetRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmForget(null);
  }

  function startRename(run: RunListEntry) {
    setRenamingRunId(run.run_id);
    setRenameValue(run.name ?? "");
    setTimeout(() => renameInputRef.current?.focus(), 0);
  }

  async function commitRename() {
    if (!renamingRunId) return;
    try {
      await renameRun(renamingRunId, renameValue.trim());
    } catch {
      // event-driven refresh will pick up state change
    }
    setRenamingRunId(null);
    setRenameValue("");
  }

  function cancelRename() {
    setRenamingRunId(null);
    setRenameValue("");
  }

  async function handleConfirmDelete() {
    if (!deleteTarget) return;
    try {
      // Forward scope so a `library` entry deletes from the library store, not
      // the same-named repo pipeline file (#216).
      await removePipeline(deleteTarget.id, deleteTarget.scope);
      if (deleteTarget.scope === "library") onLibraryPipelinesChanged();
    } catch {
      // ignore
    }
    setDeleteTarget(null);
  }

  const tabs: { id: LeftTab; label: string }[] = [
    { id: "runs", label: "Runs" },
    { id: "triggers", label: "Triggers" },
    { id: "library", label: "Library" },
  ];

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      {/* Three-tab strip: Runs · Triggers · Library (#160) */}
      <div role="tablist" className="flex h-[36px] shrink-0 items-stretch border-b border-line">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            role="tab"
            aria-selected={activeTab === tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex-1 cursor-pointer border-b-2 font-medium transition-colors ${
              activeTab === tab.id
                ? "border-acc text-fg"
                : "border-transparent text-fg-4 hover:text-fg-2"
            }`}
            style={{ fontSize: "11.5px" }}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Runs pane */}
      {activeTab === "runs" && (
        <div className="flex min-h-0 flex-1 flex-col" role="tabpanel">
          <div
            className="flex h-[32px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
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
            const { dot } = STATUS_STYLES[run.status] ?? STATUS_STYLES.running;
            const isArchived = run.status === "archived";
            const canCleanup = !isArchived;
            const isRenaming = renamingRunId === run.run_id;

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
                    run.status === "running" ? "animate-pulse" : ""
                  }`}
                />
                <div className="min-w-0 flex-1">
                  {isRenaming ? (
                    <input
                      ref={renameInputRef}
                      className="w-full rounded border border-acc bg-bg-3 px-1 py-0.5 font-medium text-fg outline-none"
                      style={{ fontSize: "11.5px" }}
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onBlur={() => commitRename()}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") commitRename();
                        if (e.key === "Escape") cancelRename();
                      }}
                      onClick={(e) => e.stopPropagation()}
                      data-testid="rename-input"
                    />
                  ) : (
                    <div className="truncate font-medium" data-testid="run-display-label">
                      {run.name || run.run_id.slice(0, 20)}
                    </div>
                  )}
                  <div
                    className="flex items-center gap-1.5 truncate font-mono text-fg-4"
                    style={{ fontSize: "10px" }}
                  >
                    <span className="truncate" data-testid="run-pipeline-name">
                      {run.pipeline_name}
                    </span>
                    {run.triggered_by && (
                      <span
                        role="button"
                        title="Created by a trigger — open the Triggers tab"
                        className="flex shrink-0 cursor-pointer items-center gap-0.5 rounded border border-acc px-1 text-acc"
                        style={{ fontSize: "9px" }}
                        data-testid="run-trigger-badge"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (run.triggered_by) onSelectTrigger?.(run.triggered_by);
                          setActiveTab("triggers");
                        }}
                      >
                        <Zap size={8} />
                        trigger
                      </span>
                    )}
                  </div>
                </div>
                {!isRenaming && (
                  <span
                    role="button"
                    title="Rename run"
                    className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
                    onClick={(e) => {
                      e.stopPropagation();
                      startRename(run);
                    }}
                    data-testid="rename-button"
                  >
                    <Pencil size={12} />
                  </span>
                )}
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
        </div>
      )}

      {/* Triggers pane (#160) */}
      {activeTab === "triggers" && (
        <div className="min-h-0 flex-1" role="tabpanel">
          <TriggersListPanel
            triggers={triggers}
            selectedTriggerId={selectedTriggerId}
            onSelectTrigger={onSelectTrigger ?? (() => {})}
            onNewTrigger={onNewTrigger ?? (() => {})}
            onTriggersChanged={onTriggersChanged ?? (() => {})}
            onRunNow={onRunNowTrigger}
            onEditTrigger={onEditTrigger}
          />
        </div>
      )}

      {/* Library pane */}
      {activeTab === "library" && (
        <div className="flex min-h-0 flex-1 flex-col" role="tabpanel">
      <div
        className="flex h-[32px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Library
        <button
          onClick={() => setShowNewModal(true)}
          className="ml-auto grid h-5 w-5 cursor-pointer place-items-center rounded border border-line-strong bg-bg-3 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
          title="New pipeline"
        >
          <Plus size={12} />
        </button>
      </div>

        <div className="flex-1 overflow-y-auto">
          {pipelines.length === 0 && libraryPipelines.length === 0 && (
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
            // A pipeline counts as "starred" when a library entry exists with
            // the same name. This is the visible link the user expects when
            // they click the canvas star: their pipeline gets a star badge
            // here, confirming the action had effect.
            const starred = libraryPipelines.some((lp) => lp.name === p.name);
            return (
              <button
                key={`${p.scope}-${p.id}`}
                onClick={() => openPipeline(p.id, p.scope)}
                className={`group flex w-full cursor-pointer items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
                  isSelected ? "bg-bg-3 text-fg" : "text-fg-2 hover:bg-bg-3/50"
                }`}
                style={{ fontSize: "11.5px" }}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    {starred && (
                      <Star
                        size={10}
                        className="shrink-0 fill-acc text-acc"
                        data-testid="left-panel-star"
                      />
                    )}
                    <span className="truncate font-medium">{p.name}</span>
                  </div>
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
          {/* Library-only entries (no matching name in /pipelines). These
              previously only showed up in the New Run dropdown — surfacing
              them here means starring a brand-new pipeline yields a visible
              entry in the sidebar, matching the user's mental model that
              starred == in the library. */}
          {libraryPipelines
            .filter((lp) => !pipelines.some((p) => p.name === lp.name))
            .map((lp) => (
              <div
                key={`lib-only-${lp.scope}-${lp.id}`}
                className="group flex w-full items-center gap-2 border-b border-line-soft px-3 py-2 text-left text-fg-2 transition-colors hover:bg-bg-3/50"
                style={{ fontSize: "11.5px" }}
                data-testid="library-only-entry"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <Star
                      size={10}
                      className="shrink-0 fill-acc text-acc"
                      data-testid="left-panel-star"
                    />
                    <span className="truncate font-medium">{lp.name}</span>
                  </div>
                  <div
                    className="mt-0.5 flex items-center gap-1.5 text-fg-4"
                    style={{ fontSize: "10px" }}
                  >
                    <span>{lp.node_count} nodes</span>
                  </div>
                </div>
                <span
                  className={`shrink-0 rounded border px-1 py-px group-hover:hidden ${SCOPE_BADGE[lp.scope].cls}`}
                  style={{ fontSize: "9px", fontWeight: 500 }}
                >
                  {SCOPE_BADGE[lp.scope].label}
                </span>
                <span
                  className="hidden shrink-0 group-hover:inline-flex"
                  data-testid="library-duplicate-button"
                  onClick={async (e) => {
                    e.stopPropagation();
                    if (duplicatingId === lp.id) return;
                    setDuplicatingId(lp.id);
                    try {
                      await duplicateLibraryPipeline(lp.id);
                      onLibraryPipelinesChanged(); // refresh; do NOT auto-open the copy
                    } catch { /* ignore */ }
                    finally { setDuplicatingId(null); }
                  }}
                  role="button"
                  title="Duplicate pipeline"
                >
                  <Copy
                    size={14}
                    className="text-fg-4 transition-colors hover:text-acc"
                  />
                </span>
                <span
                  className="hidden shrink-0 group-hover:inline-flex"
                  onClick={async (e) => {
                    e.stopPropagation();
                    try {
                      await deleteLibraryPipeline(lp.id);
                      onLibraryPipelinesChanged();
                    } catch { /* ignore */ }
                  }}
                  role="button"
                  title="Remove from library"
                >
                  <Trash2
                    size={14}
                    className="text-fg-4 transition-colors hover:text-st-failed"
                  />
                </span>
              </div>
            ))}
        </div>
        </div>
      )}

      {confirmCleanup && (
        <CleanupConfirmModal
          runId={confirmCleanup.runId}
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
