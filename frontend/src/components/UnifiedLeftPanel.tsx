import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Copy, FileUp, Pause, Pencil, Play, Plus, RotateCcw, SquareTerminal, Star, Trash2, Zap } from "lucide-react";
import { isLiveRun, isTerminalRun, type RunListEntry, type RunStatus, type PipelineListEntry, type PipelineScope, type Trigger } from "../types";
import type { LibraryPipelineEntry } from "../api";
import { cleanupRun, createPipeline, deleteLibraryPipeline, duplicateLibraryPipeline, forgetRun, importWorkflow, openRunShell, pauseRun, renameRun, resumeRun, retryAll } from "../api";
import { useEditStore } from "../stores/editStore";
import { groupByRepo } from "../lib/groupByRepo";
import CleanupConfirmModal from "./CleanupConfirmModal";
import ConfirmDeleteModal from "./ConfirmDeleteModal";
import ForgetRunModal from "./ForgetRunModal";
import RunFilters, { EMPTY_RUN_FILTER, runMatchesFilter } from "./RunFilters";
import RunShellModal from "./RunShellModal";
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
  // #110 — run id awaiting Retry-all confirmation (the one destructive control).
  const [confirmRetryAll, setConfirmRetryAll] = useState<string | null>(null);
  // Ad-hoc bash shell opened on a terminal run (#316). Holds the tmux session
  // name to attach the inline terminal to.
  const [shellRun, setShellRun] = useState<{ runId: string; session: string } | null>(null);
  const [renamingRunId, setRenamingRunId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  const pipelines = useEditStore((s) => s.pipelines);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const removePipeline = useEditStore((s) => s.removePipeline);
  const activeTabId = useEditStore((s) => s.activeTabId);

  const [showNewModal, setShowNewModal] = useState(false);
  const [showImportModal, setShowImportModal] = useState(false);
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

  // #110 — Pause/Resume are cheap, reversible, fire-and-forget (silent catch like
  // handleCleanup); the daemon's WS events re-drive the list state.
  async function handlePause(runId: string) {
    try {
      await pauseRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
  }

  async function handleResume(runId: string) {
    try {
      await resumeRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
  }

  // #110 — Retry-all is destructive (archives the original), so it's confirm-gated.
  // The daemon replies 201 with the offspring run_id; selecting it fires an
  // independent fetch-by-id (App's handleSelectRun) — safe before the WS refresh
  // lands the new row in `runs`.
  async function handleRetryAll(runId: string) {
    try {
      const { run_id } = await retryAll(runId);
      onSelectRun(run_id);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmRetryAll(null);
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

  async function handleConfirmDelete(cascade: boolean) {
    if (!deleteTarget) return;
    // Match on `name`, never id: the Library copy's id is an independently
    // derived slug that can diverge from the repo pipeline's file-stem id, and
    // the whole star/twin model is name-keyed (#227).
    const twins = libraryPipelines.filter((lp) => lp.name === deleteTarget.name);
    const cascadable = deleteTarget.scope !== "library" && twins.length === 1;
    try {
      // Forward scope so a `library` entry deletes from the library store, not
      // the same-named repo pipeline file (#216).
      await removePipeline(deleteTarget.id, deleteTarget.scope);
      if (cascade && cascadable) {
        // #227: also remove the durable Library copy the star created.
        try {
          await deleteLibraryPipeline(twins[0].id);
        } catch {
          /* non-fatal: the working pipeline is already gone */
        }
      }
      // Re-fetch the authoritative block-1 list (covers the #216 dual-scope row).
      await loadPipelines();
    } catch {
      // ignore (e.g. 409 active runs)
    } finally {
      // #227 core: refresh the library list on EVERY delete, not only
      // scope === "library" — otherwise a deleted repo star's copy lingers
      // and re-surfaces as a phantom library-only row.
      onLibraryPipelinesChanged();
      setDeleteTarget(null);
    }
  }

  // One run row, rendered identically whether the list is flat or grouped by
  // repo (#258). Extracted so both code paths share the exact same markup.
  function renderRunRow(run: RunListEntry) {
    const isSelected = run.run_id === selectedRunId;
    const { dot } = STATUS_STYLES[run.status] ?? STATUS_STYLES.running;
    const isArchived = run.status === "archived";
    const canCleanup = !isArchived;
    const isRenaming = renamingRunId === run.run_id;
    // #110 — gate on EXPLICIT statuses, never isLiveRun/isTerminalRun: the former
    // includes `paused` (would wrongly show Pause on a paused run → 409); the
    // latter includes `archived` (would wrongly show Retry-all on archived → 409).
    // Each set is a subset of its daemon guard's accepted statuses.
    const canPause = run.status === "running" || run.status === "awaiting_user";
    const canResume = run.status === "paused";
    const canRetryAll =
      run.status === "completed" ||
      run.status === "failed" ||
      run.status === "halted" ||
      run.status === "skipped";

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
        {canPause && (
          <span
            role="button"
            title="Pause run"
            data-testid="pause-run-button"
            className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
            onClick={(e) => {
              e.stopPropagation();
              handlePause(run.run_id);
            }}
          >
            <Pause size={12} />
          </span>
        )}
        {canResume && (
          <span
            role="button"
            title="Resume run"
            data-testid="resume-run-button"
            className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
            onClick={(e) => {
              e.stopPropagation();
              handleResume(run.run_id);
            }}
          >
            <Play size={12} />
          </span>
        )}
        {canRetryAll && (
          <span
            role="button"
            title="Retry all — archive this run and start a fresh one"
            data-testid="retry-all-button"
            className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
            onClick={(e) => {
              e.stopPropagation();
              setConfirmRetryAll(run.run_id);
            }}
          >
            <RotateCcw size={12} />
          </span>
        )}
        {isTerminalRun(run.status) && !isArchived && (
          <span
            role="button"
            title="Open a bash shell in this run's worktree"
            data-testid="open-session-button"
            className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
            onClick={async (e) => {
              e.stopPropagation();
              try {
                const { session } = await openRunShell(run.run_id);
                setShellRun({ runId: run.run_id, session });
              } catch {
                // Silent, like handleCleanup — the server gate may 409 if the
                // worktree vanished out-of-band; nothing actionable in the row.
              }
            }}
          >
            <SquareTerminal size={12} />
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
  }

  // #336 — client-side run filters (project / pipeline / trigger), AND
  // semantics, session-only state. Applied to `runs` BEFORE the active/archived
  // split so grouping, the Archived section and its count all see the same
  // filtered view.
  const [runFilter, setRunFilter] = useState(EMPTY_RUN_FILTER);
  const filteredRuns = useMemo(
    () => runs.filter((r) => runMatchesFilter(r, runFilter)),
    [runs, runFilter],
  );
  const filterActive =
    runFilter.repo !== null || runFilter.pipeline !== null || runFilter.trigger !== null;

  // #136 — archived runs live in their own flat, collapsible section below the
  // active list; the active list keeps the #258 per-repo grouping.
  const activeRuns = filteredRuns.filter((r) => r.status !== "archived");
  const archivedRuns = filteredRuns.filter((r) => r.status === "archived");

  // Identity of the selected run *iff* it currently sits in the archived set,
  // else null — the signal that must reveal the section.
  const selectedArchivedId =
    selectedRunId != null && archivedRuns.some((r) => r.run_id === selectedRunId)
      ? selectedRunId
      : null;

  // Collapsed by default; expanded on mount only if the selected run is already
  // archived (so it's visible on first paint).
  const [archivedOpen, setArchivedOpen] = useState(() => selectedArchivedId !== null);

  // Auto-expand when the selected-archived run *changes* (a live run archived
  // mid-session while selected, or selecting a different archived run). Adjusting
  // state during render on a tracked-key change — React's reset-on-prop pattern,
  // cf. App.tsx `lastCanvasFocus` and useDismissedNudges `prevTabId` — fires the
  // reveal exactly once per transition, so a later chevron collapse sticks (no
  // dead-lock). Force-open only; never force-close.
  const [prevSelectedArchivedId, setPrevSelectedArchivedId] = useState(selectedArchivedId);
  if (prevSelectedArchivedId !== selectedArchivedId) {
    setPrevSelectedArchivedId(selectedArchivedId);
    if (selectedArchivedId !== null) setArchivedOpen(true);
  }

  // Group the active Runs list by project (#258) only when ≥ 2 distinct repos are
  // present; otherwise `null` ⇒ the flat list, byte-identical to before.
  const runGroups = groupByRepo(activeRuns, (r) => r.effective_repo);

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
        {runs.length > 0 && (
          <RunFilters
            runs={runs}
            triggers={triggers}
            value={runFilter}
            onChange={setRunFilter}
          />
        )}
        <div className="flex-1 overflow-y-auto">
          {runs.length === 0 && (
            <div
              className="px-3 py-4 text-center text-fg-4"
              style={{ fontSize: "11px" }}
            >
              No runs yet
            </div>
          )}
          {runs.length > 0 && filterActive && filteredRuns.length === 0 && (
            <div
              className="px-3 py-4 text-center text-fg-4"
              style={{ fontSize: "11px" }}
              data-testid="run-filter-empty"
            >
              No runs match filters
              <button
                data-testid="run-filter-empty-clear"
                className="mt-1 block w-full cursor-pointer text-acc hover:underline"
                onClick={() => setRunFilter(EMPTY_RUN_FILTER)}
              >
                Clear filters
              </button>
            </div>
          )}
          {runGroups === null
            ? activeRuns.map(renderRunRow)
            : runGroups.map((group) => (
                <div key={group.repoPath} data-testid="run-repo-group">
                  <div
                    className="flex h-[22px] shrink-0 items-center border-b border-line-soft bg-bg-3/40 px-3 font-medium text-fg-3"
                    style={{ fontSize: "10px" }}
                    title={group.repoPath}
                  >
                    <span className="truncate" data-testid="run-repo-label">
                      {group.label}
                    </span>
                  </div>
                  {group.items.map(renderRunRow)}
                </div>
              ))}
          {/* #136 — archived runs in their own flat, collapsible section below
              the active list. Reuses renderRunRow verbatim (same archived
              styling / Forget action). The rendered gate is `archivedOpen`
              ALONE — never `archivedOpen || some(selected)`, which dead-locks
              the chevron while a selected run is archived (see decision 4). */}
          {archivedRuns.length > 0 && (
            <div data-testid="run-archived-section" className="border-t border-line">
              <button
                onClick={() => setArchivedOpen((o) => !o)}
                className="flex w-full items-center gap-1.5 px-3 py-2 text-fg-2 transition-colors hover:bg-bg-3 cursor-pointer"
                style={{ fontSize: "11.5px" }}
                data-testid="run-archived-toggle"
              >
                {archivedOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                <span className="font-medium">
                  Archived <span data-testid="run-archived-count">({archivedRuns.length})</span>
                </span>
              </button>
              {archivedOpen && archivedRuns.map(renderRunRow)}
            </div>
          )}
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
          onClick={() => setShowImportModal(true)}
          className="ml-auto grid h-5 w-5 cursor-pointer place-items-center rounded border border-line-strong bg-bg-3 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
          title="Import a workflow"
          data-testid="import-workflow-button"
        >
          <FileUp size={12} />
        </button>
        <button
          onClick={() => setShowNewModal(true)}
          className="ml-1.5 grid h-5 w-5 cursor-pointer place-items-center rounded border border-line-strong bg-bg-3 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
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
                {/* #273: scope:"library" rows now appear here in block 1 (the
                    /pipelines scope-merge from #216 means they no longer fall
                    through to the library-only block below). Surface the same
                    Copy affordance #224 shipped, gated on identity (scope), not
                    the name-absence filter that block 2 uses. `p.id` is the HOME
                    library file-stem — duplicateLibraryPipeline resolves it. */}
                {p.scope === "library" && (
                  <span
                    className="hidden shrink-0 group-hover:inline-flex"
                    data-testid="library-duplicate-button"
                    onClick={async (e) => {
                      e.stopPropagation();
                      if (duplicatingId === p.id) return;
                      setDuplicatingId(p.id);
                      try {
                        await duplicateLibraryPipeline(p.id);
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
                )}
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

      {shellRun && (
        <RunShellModal
          session={shellRun.session}
          onClose={() => setShellRun(null)}
        />
      )}

      {confirmRetryAll && (
        <RetryAllConfirmModal
          onConfirm={() => handleRetryAll(confirmRetryAll)}
          onCancel={() => setConfirmRetryAll(null)}
        />
      )}

      {(() => {
        // Show the cascade checkbox only when the target has exactly one
        // same-name Library copy and isn't itself the library row (#227).
        const twins = deleteTarget
          ? libraryPipelines.filter((lp) => lp.name === deleteTarget.name)
          : [];
        const cascadable =
          deleteTarget != null &&
          deleteTarget.scope !== "library" &&
          twins.length === 1;
        return (
          <ConfirmDeleteModal
            // Remount per target so the checkbox resets to OFF each open (#227).
            key={deleteTarget?.id ?? "none"}
            open={deleteTarget !== null}
            onClose={() => setDeleteTarget(null)}
            onConfirm={handleConfirmDelete}
            name={deleteTarget?.name ?? ""}
            cascadeLabel={cascadable ? "Also remove the Library copy" : undefined}
          />
        );
      })()}

      {showNewModal && (
        <NewPipelineModal onClose={() => setShowNewModal(false)} />
      )}

      {showImportModal && (
        <ImportWorkflowModal
          onClose={() => setShowImportModal(false)}
          onImported={onLibraryPipelinesChanged}
        />
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

/// Import a Claude Code workflow `.js` as a draft library pipeline (#155). The
/// file is read client-side and POSTed as text — the daemon parses it to an AST
/// (never executes it) and returns a draft plus lossy-translation warnings.
function ImportWorkflowModal({
  onClose,
  onImported,
}: {
  onClose: () => void;
  onImported: () => void;
}) {
  const [file, setFile] = useState<File | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[] | null>(null);
  const loadPipelines = useEditStore((s) => s.loadPipelines);

  async function handleImport() {
    if (!file || submitting) return;
    setSubmitting(true);
    setError(null);
    setWarnings(null);
    try {
      const content = await file.text();
      const result = await importWorkflow(file.name, content);
      onImported();
      await loadPipelines();
      const w = result.warnings ?? [];
      if (w.length > 0) {
        // Surface the lossy-translation diagnostics (ADR-0001) rather than
        // silently closing — the annotation is the onboarding tutorial.
        setWarnings(w);
      } else {
        onClose();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        className="w-[400px] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
        data-testid="import-workflow-modal"
      >
        <div className="mb-1 font-medium text-fg">Import a workflow</div>
        <p className="mb-3 text-fg-4" style={{ fontSize: "11px" }}>
          Decompile a Claude Code workflow (<code>.js</code>) into a draft
          pipeline. The file is parsed, never run; unmapped idioms become
          annotated placeholders.
        </p>

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Workflow file
        </label>
        <input
          type="file"
          accept=".js"
          data-testid="workflow-file-input"
          onChange={(e) => {
            setFile(e.target.files?.[0] ?? null);
            setError(null);
            setWarnings(null);
          }}
          className="mb-3 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none file:mr-2 file:rounded file:border-0 file:bg-bg-4 file:px-2 file:py-0.5 file:text-fg-3"
        />

        {error && (
          <div
            className="mb-3 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-st-failed"
            style={{ fontSize: "11px" }}
            data-testid="import-workflow-error"
          >
            {error}
          </div>
        )}

        {warnings && (
          <div
            className="mb-3 max-h-40 overflow-y-auto rounded border border-st-await/40 bg-st-await/10 px-2 py-1.5 text-fg-2"
            style={{ fontSize: "11px" }}
            data-testid="import-workflow-warnings"
          >
            <div className="mb-1 font-medium text-st-await">
              Imported with {warnings.length} translation warning
              {warnings.length === 1 ? "" : "s"}:
            </div>
            <ul className="list-disc pl-4">
              {warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          >
            {warnings ? "Done" : "Cancel"}
          </button>
          {!warnings && (
            <button
              onClick={handleImport}
              disabled={!file || submitting}
              className="rounded bg-acc px-3 py-1 font-medium text-bg-0 transition-colors hover:bg-acc-dim disabled:opacity-50"
            >
              {submitting ? "Importing…" : "Import"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/// #110 — confirm dialog for the one destructive run-level control. Retry-all
/// archives the current run and starts a fresh run of the same pipeline, so it
/// gets a confirm gate (Pause/Resume don't — they're cheap and reversible).
function RetryAllConfirmModal({
  onConfirm,
  onCancel,
}: {
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="retry-all-backdrop"
      onClick={onCancel}
    >
      <div
        className="w-[360px] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1 font-medium text-fg">Retry all nodes?</div>
        <p className="mb-4 text-fg-3" style={{ fontSize: "11px" }}>
          This archives the current run and starts a fresh run of the same pipeline
          from the beginning. The archived run stays viewable (read-only).
        </p>
        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            data-testid="retry-all-confirm-button"
            className="rounded bg-acc px-3 py-1 font-medium text-bg-0 transition-colors hover:bg-acc-dim"
          >
            Retry all
          </button>
        </div>
      </div>
    </div>
  );
}
