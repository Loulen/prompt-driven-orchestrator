import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Copy, FileUp, Pause, Pencil, Play, Plus, RotateCcw, SquareTerminal, Trash2, Zap } from "lucide-react";
import { isLiveRun, isTerminalRun, type RunListEntry, type RunStatus, type PipelineListEntry, type PipelineScope, type Trigger, type Project } from "../types";
import type { LibraryPipelineEntry } from "../api";
import { cleanupRun, createPipeline, deleteLibraryPipeline, duplicateLibraryPipeline, forgetRun, importWorkflow, openRunShell, pauseRun, renameRun, resumeRun, retryAll } from "../api";
import { useEditStore } from "../stores/editStore";
import { useSelectionStore } from "../stores/selectionStore";
import { handleSelectionKeydown } from "../lib/selectionKeys";
import { type BulkItem, type BulkOutcome } from "../lib/bulk";
import { groupByProject, type ProjectRef } from "../lib/groupByRepo";
import { projectLookup } from "../lib/projectLookup";
import { cascadableTwin, isStarred, libraryOnly } from "../lib/libraryTwins";
import BulkActionBar from "./BulkActionBar";
import BulkActionModal from "./BulkActionModal";
import CleanupConfirmModal from "./CleanupConfirmModal";
import ConfirmDeleteModal from "./ConfirmDeleteModal";
import ForgetRunModal from "./ForgetRunModal";
import LibraryRow from "./LibraryRow";
import ProjectEditModal from "./ProjectEditModal";
import RunFilters from "./RunFilters";
import { EMPTY_RUN_FILTER, runMatchesFilter } from "./runFilter";
import RunShellModal from "./RunShellModal";
import SelectControl from "./SelectControl";
import TriggersListPanel from "./TriggersListPanel";

type LeftTab = "runs" | "triggers" | "library";

const STATUS_STYLES: Record<RunStatus, { dot: string; ring: string }> = {
  running: { dot: "bg-st-running", ring: "border-st-running" },
  awaiting_user: { dot: "bg-st-await", ring: "border-st-await" },
  completed: { dot: "bg-st-done", ring: "border-st-done" },
  failed: { dot: "bg-st-failed", ring: "border-st-failed" },
  skipped: { dot: "bg-st-skipped", ring: "border-st-skipped" },
  halted: { dot: "bg-st-blocked", ring: "border-st-blocked" },
  paused: { dot: "bg-st-paused", ring: "border-st-paused" },
  archived: { dot: "bg-st-archived", ring: "border-st-archived" },
};

/** Runs whose bulk-Retry is valid (mirror of the per-row `canRetryAll`). */
const RETRYABLE: readonly RunStatus[] = ["completed", "failed", "halted", "skipped"];

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
  /** #348 global Trigger kill-switch state + toggle. Optional so existing
   * callers/tests keep working (defaults to not-paused, no-op toggle). */
  triggersPaused?: boolean;
  onTogglePause?: () => void;
  /** Projets (#552) — the group-by-Projet layer and the pencil's source of
   *  truth. Optional so existing callers/tests keep working (no Projet → the
   *  #258 per-path grouping, unchanged). */
  projects?: Project[];
  onProjectsChanged?: () => void;
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
  triggersPaused = false,
  onTogglePause,
  projects = [],
  onProjectsChanged,
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
  // #552 — the group-header pencil. Holds the Projet being edited (or `null` for
  // a fresh one, when the header is a derived path group) plus the pre-fill.
  const [projectEditor, setProjectEditor] = useState<{
    project: Project | null;
    name: string;
    memberPaths: string[];
  } | null>(null);

  const pipelines = useEditStore((s) => s.pipelines);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const removePipeline = useEditStore((s) => s.removePipeline);
  const activeTabId = useEditStore((s) => s.activeTabId);

  // #577 — multi-select. The per-tab sets live in the shared store (so a tab's
  // count survives a switch as its badge); the pending bulk action is local.
  const runSelIds = useSelectionStore((s) => s.runs);
  const librarySelIds = useSelectionStore((s) => s.library);
  const triggerSelCount = useSelectionStore((s) => s.triggers.length);
  const toggleSel = useSelectionStore((s) => s.toggle);
  const selectRange = useSelectionStore((s) => s.selectRange);
  const selectVisible = useSelectionStore((s) => s.selectVisible);
  const selectGroup = useSelectionStore((s) => s.selectGroup);
  const deselect = useSelectionStore((s) => s.deselect);
  const clearSel = useSelectionStore((s) => s.clear);
  const runSel = useMemo(() => new Set(runSelIds), [runSelIds]);
  const librarySel = useMemo(() => new Set(librarySelIds), [librarySelIds]);
  const [runBulkKind, setRunBulkKind] = useState<"cleanup" | "retry" | "pause" | null>(null);
  const [libBulkKind, setLibBulkKind] = useState<"delete" | "duplicate" | null>(null);

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
    // The twin rule (name-keyed, unique-only) lives in `lib/libraryTwins` — the
    // same call the checkbox's visibility uses, so what the user was offered and
    // what runs here cannot drift (#227).
    const twin = cascadableTwin(deleteTarget, libraryPipelines);
    try {
      // Forward scope so a `library` entry deletes from the library store, not
      // the same-named repo pipeline file (#216).
      await removePipeline(deleteTarget.id, deleteTarget.scope);
      if (cascade && twin) {
        // #227: also remove the durable Library copy the star created.
        try {
          await deleteLibraryPipeline(twin.id);
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

  // #224/#371 — duplicate a library template. One shared seam for both Copy
  // affordances (the block-1 scope:"library" row and the block-2 library-only
  // row) so they can never again drift apart. Busy-guarded (a double-click on
  // the Copy icon fires once), then refreshes BOTH pipeline lists:
  //   - loadPipelines() re-fetches the authoritative /pipelines, where the
  //     daemon tags the copy scope:"library" — so it lands in the proper
  //     block-1 button path at once: a clickable, correctly-badged row.
  //   - onLibraryPipelinesChanged() re-fetches /library/pipelines.
  // Refreshing only the latter (the pre-#371 behaviour) left the copy in the
  // degraded block-2 <div>: wrong "user" badge, no button role, dead click,
  // until a full page reload. The New/Import handlers already loadPipelines()
  // the same way; only Duplicate had drifted.
  async function handleDuplicate(id: string) {
    if (duplicatingId === id) return;
    setDuplicatingId(id);
    try {
      await duplicateLibraryPipeline(id);
      await loadPipelines();
      onLibraryPipelinesChanged(); // refresh; do NOT auto-open the copy
    } catch {
      /* ignore */
    } finally {
      setDuplicatingId(null);
    }
  }

  // One run row, rendered identically whether the list is flat or grouped by
  // repo (#258). Extracted so both code paths share the exact same markup.
  function renderRunRow(run: RunListEntry) {
    const isSelected = run.run_id === selectedRunId;
    const rowSelected = runSel.has(run.run_id);
    // A stalled run (no node running/waiting, nothing schedulable; #180) is
    // surfaced amber and steady, overriding its still-`running` canonical
    // status — "never a silent stall". `stalled` is derived per read by the
    // daemon (`event_log::is_stalled`) and shipped on every list entry.
    const dot = run.stalled
      ? "bg-st-stale"
      : (STATUS_STYLES[run.status] ?? STATUS_STYLES.running).dot;
    const ring = run.stalled
      ? "border-st-stale"
      : (STATUS_STYLES[run.status] ?? STATUS_STYLES.running).ring;
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
        className={`group flex w-full cursor-pointer items-center gap-2 border-b border-l-2 border-line-soft px-3 py-2 text-left transition-colors ${
          rowSelected
            ? "border-l-acc bg-acc-bg text-fg"
            : isSelected
              ? "border-l-transparent bg-bg-3 text-fg"
              : "border-l-transparent text-fg-2 hover:bg-bg-3/50"
        } ${isArchived ? "opacity-60" : ""}`}
        style={{ fontSize: "11.5px" }}
      >
        {/* #577 — the status dot doubles as the select control: it goes hollow on
            hover and becomes a green check when selected. #503: the resting dot
            still carries the failure reason so a red Run has something to say. */}
        <SelectControl
          selected={rowSelected}
          dotClass={dot}
          ringClass={ring}
          pulse={run.status === "running" && !run.stalled}
          dotTitle={run.failure_reason ?? undefined}
          dotTestId="run-status-dot"
          label={rowSelected ? `Deselect ${run.name || run.run_id}` : `Select ${run.name || run.run_id}`}
          onSelect={(e) => {
            if (e.shiftKey) selectRange("runs", run.run_id, visibleRunIds);
            else toggleSel("runs", run.run_id);
          }}
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

  // #552 — verbatim `path → Projet` lookup, and the candidate repos the pencil
  // can attach (the distinct effective repos across ALL runs, so a filtered view
  // never hides an attachable repo).
  const projectOf = useMemo<(path: string) => ProjectRef | null>(
    () => projectLookup(projects),
    [projects],
  );
  const availableRepos = useMemo(() => {
    const set = new Set<string>();
    for (const r of runs) if (r.effective_repo) set.add(r.effective_repo);
    return [...set];
  }, [runs]);

  // Group the active Runs list by Projet (#552), falling back to the #258
  // per-path grouping when nothing is named; `null` ⇒ the flat list.
  const runGroups = groupByProject(activeRuns, (r) => r.effective_repo, projectOf);

  // #577 — Runs multi-select derivations. `visibleRunIds` is the flattened visible
  // order (grouped active list, then the archived section only when expanded) —
  // the basis for a shift-range and for select-all-visible.
  const visibleRunIds = [
    ...(runGroups === null ? activeRuns : runGroups.flatMap((g) => g.items)).map((r) => r.run_id),
    ...(archivedOpen ? archivedRuns.map((r) => r.run_id) : []),
  ];
  const selectedRuns = filteredRuns.filter((r) => runSel.has(r.run_id));
  const asRunItem = (r: RunListEntry): BulkItem => ({ id: r.run_id, label: r.name || r.run_id });
  // Each bulk action targets the subset of the selection it is valid for (mirror
  // of the per-row gates); an action whose subset is empty is disabled.
  const cleanupItems = selectedRuns.filter((r) => r.status !== "archived").map(asRunItem);
  const retryItems = selectedRuns.filter((r) => RETRYABLE.includes(r.status)).map(asRunItem);
  const pauseItems = selectedRuns
    .filter((r) => r.status === "running" || r.status === "awaiting_user")
    .map(asRunItem);
  // Live runs the cleanup would stop (running/awaiting/paused) — the bar's caveat.
  const runningWillStop = selectedRuns.filter((r) => isLiveRun(r.status)).length;
  const handleRunsSettled = useCallback(
    (o: BulkOutcome) => deselect("runs", o.succeeded.map((r) => r.id)),
    [deselect],
  );

  // #577 — Library multi-select. The two Library lists (openable /pipelines rows
  // and passive library-only rows) delete through different seams, so each row
  // registers its own `del`/`dup` keyed by the SAME id its React key uses. Bulk
  // then just dispatches per selected id — no re-deriving which list it came from.
  const libraryOnlyEntries = libraryOnly(libraryPipelines, pipelines);
  interface LibTarget { selId: string; name: string; del: () => Promise<void>; dup?: () => Promise<void>; }
  const libTargets: LibTarget[] = [
    ...pipelines.map((p) => ({
      selId: `${p.scope}-${p.id}`,
      name: p.name,
      del: () => removePipeline(p.id, p.scope),
      // Duplicate is a library operation — offered only on a library-scoped row
      // (same rule as the per-row Copy affordance, #224/#273).
      dup: p.scope === "library" ? () => duplicateLibraryPipeline(p.id).then(() => {}) : undefined,
    })),
    ...libraryOnlyEntries.map((lp) => ({
      selId: `lib-only-${lp.scope}-${lp.id}`,
      name: lp.name,
      del: () => deleteLibraryPipeline(lp.id),
      dup: () => duplicateLibraryPipeline(lp.id).then(() => {}),
    })),
  ];
  const libTargetById = new Map(libTargets.map((t) => [t.selId, t]));
  const libVisibleIds = libTargets.map((t) => t.selId);
  const selectedLibTargets = libTargets.filter((t) => librarySel.has(t.selId));
  const libDeleteItems: BulkItem[] = selectedLibTargets.map((t) => ({ id: t.selId, label: t.name }));
  const libDupItems: BulkItem[] = selectedLibTargets
    .filter((t) => t.dup)
    .map((t) => ({ id: t.selId, label: t.name }));
  const handleLibrarySettled = useCallback(
    (o: BulkOutcome) => {
      deselect("library", o.succeeded.map((r) => r.id));
      // Refresh BOTH pipeline lists on every bulk op (delete / duplicate), the
      // same pair the single-item paths refresh (#227/#371).
      void loadPipelines();
      onLibraryPipelinesChanged();
    },
    [deselect, loadPipelines, onLibraryPipelinesChanged],
  );

  // Open the pencil on a group header: an existing Projet pre-fills its record;
  // a derived path group pre-fills the label + its own path as the sole member.
  const openProjectEditor = (group: {
    kind: "project" | "path";
    key: string;
    repoPath: string;
    label: string;
  }) => {
    if (group.kind === "project") {
      const id = group.key.slice("project:".length);
      const project = projects.find((p) => p.id === id) ?? null;
      setProjectEditor({
        project,
        name: project?.name ?? group.label,
        memberPaths: project?.members ?? [],
      });
    } else {
      setProjectEditor({
        project: null,
        name: group.label,
        memberPaths: group.repoPath ? [group.repoPath] : [],
      });
    }
  };

  const tabs: { id: LeftTab; label: string }[] = [
    { id: "runs", label: "Runs" },
    { id: "triggers", label: "Triggers" },
    { id: "library", label: "Library" },
  ];
  // #577 — per-tab selection counts, for the badge left on the tab you switch away
  // from (so an in-flight selection is never silently lost).
  const selCounts: Record<LeftTab, number> = {
    runs: runSelIds.length,
    triggers: triggerSelCount,
    library: librarySelIds.length,
  };

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
            className={`flex flex-1 cursor-pointer items-center justify-center gap-1.5 border-b-2 font-medium transition-colors ${
              activeTab === tab.id
                ? "border-acc text-fg"
                : "border-transparent text-fg-4 hover:text-fg-2"
            }`}
            style={{ fontSize: "11.5px" }}
          >
            {tab.label}
            {/* #577 — count badge left on a NON-active tab with a live selection. */}
            {activeTab !== tab.id && selCounts[tab.id] > 0 && (
              <span
                data-testid={`tab-badge-${tab.id}`}
                className="rounded-full bg-acc px-1.5 font-semibold text-[#04140d]"
                style={{ fontSize: "9px" }}
              >
                {selCounts[tab.id]}
              </span>
            )}
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
        <div
          className="flex-1 overflow-y-auto outline-none"
          tabIndex={-1}
          onKeyDown={(e) =>
            handleSelectionKeydown(e, {
              tab: "runs",
              visibleIds: visibleRunIds,
              hasSelection: runSelIds.length > 0,
              selectVisible,
              clear: clearSel,
              onBulkDelete: () => {
                if (cleanupItems.length > 0) setRunBulkKind("cleanup");
              },
            })
          }
        >
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
                <div
                  key={group.key}
                  data-testid="run-repo-group"
                  data-project={group.kind === "project" ? "true" : "false"}
                >
                  <div
                    className="group group/hdr flex h-[22px] shrink-0 items-center gap-1 border-b border-line-soft bg-bg-3/40 px-3 font-medium text-fg-3"
                    style={{ fontSize: "10px" }}
                    title={group.title}
                  >
                    {/* #577 — "select all in this repo" (reveals on header hover). */}
                    <SelectControl
                      selected={
                        group.items.length > 0 &&
                        group.items.every((r) => runSel.has(r.run_id))
                      }
                      label={`Select all in ${group.label}`}
                      onSelect={() => selectGroup("runs", group.items.map((r) => r.run_id))}
                      testId="run-group-select-all"
                    />
                    <span className="truncate" data-testid="run-repo-label">
                      {group.label}
                    </span>
                    {/* #552 — the pencil that names / renames the Projet. */}
                    <button
                      onClick={() => openProjectEditor(group)}
                      className="ml-auto hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover/hdr:inline-flex"
                      title={group.kind === "project" ? "Edit project" : "Name project"}
                      aria-label={group.kind === "project" ? "Edit project" : "Name project"}
                      data-testid="run-group-pencil"
                    >
                      <Pencil size={10} />
                    </button>
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

        {/* #577 — floating bulk-action bar for the Runs tab. */}
        {runSelIds.length > 0 && (
          <BulkActionBar
            count={runSelIds.length}
            note={runningWillStop > 0 ? `${runningWillStop} running will stop` : null}
            actions={[
              {
                key: "cleanup",
                label: "Cleanup",
                icon: <Trash2 size={13} />,
                destructive: true,
                disabled: cleanupItems.length === 0,
                onClick: () => setRunBulkKind("cleanup"),
              },
              {
                key: "retry",
                label: "Retry",
                icon: <RotateCcw size={13} />,
                disabled: retryItems.length === 0,
                onClick: () => setRunBulkKind("retry"),
              },
              {
                key: "pause",
                label: "Pause",
                icon: <Pause size={13} />,
                disabled: pauseItems.length === 0,
                onClick: () => setRunBulkKind("pause"),
              },
            ]}
            onClear={() => clearSel("runs")}
          />
        )}
        {runBulkKind === "cleanup" && (
          <BulkActionModal
            destructive
            runningLabel="Cleaning up"
            title={`Cleanup ${cleanupItems.length} run${cleanupItems.length === 1 ? "" : "s"}?`}
            description={
              <>
                This removes the selected runs' worktrees from disk and archives them.
                {runningWillStop > 0
                  ? ` ${runningWillStop} running run${runningWillStop === 1 ? "" : "s"} will be stopped first.`
                  : ""}{" "}
                Completed outputs are kept — each run stays viewable (read-only). This can't be
                undone.
              </>
            }
            confirmLabel="Cleanup"
            items={cleanupItems}
            run={(id) => cleanupRun(id)}
            onClose={() => setRunBulkKind(null)}
            onSettled={handleRunsSettled}
          />
        )}
        {runBulkKind === "retry" && (
          <BulkActionModal
            destructive
            runningLabel="Retrying"
            title={`Retry ${retryItems.length} run${retryItems.length === 1 ? "" : "s"}?`}
            description="This archives each selected run and starts a fresh run of the same pipeline. The archived runs stay viewable (read-only)."
            confirmLabel="Retry all"
            items={retryItems}
            run={(id) => retryAll(id).then(() => {})}
            onClose={() => setRunBulkKind(null)}
            onSettled={handleRunsSettled}
          />
        )}
        {runBulkKind === "pause" && (
          <BulkActionModal
            skipConfirm
            runningLabel="Pausing"
            title=""
            description=""
            confirmLabel="Pause"
            items={pauseItems}
            run={(id) => pauseRun(id)}
            onClose={() => setRunBulkKind(null)}
            onSettled={handleRunsSettled}
          />
        )}
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
            paused={triggersPaused}
            onTogglePause={onTogglePause ?? (() => {})}
            projects={projects}
            onProjectsChanged={onProjectsChanged}
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

        <div
          className="flex-1 overflow-y-auto outline-none"
          tabIndex={-1}
          onKeyDown={(e) =>
            handleSelectionKeydown(e, {
              tab: "library",
              visibleIds: libVisibleIds,
              hasSelection: librarySelIds.length > 0,
              selectVisible,
              clear: clearSel,
              onBulkDelete: () => {
                if (libDeleteItems.length > 0) setLibBulkKind("delete");
              },
            })
          }
        >
          {pipelines.length === 0 && libraryPipelines.length === 0 && (
            <div
              className="px-3 py-4 text-center text-fg-4"
              style={{ fontSize: "11px" }}
            >
              No pipelines found
            </div>
          )}
          {pipelines.map((p) => (
            <LibraryRow
              key={`${p.scope}-${p.id}`}
              name={p.name}
              scope={p.scope}
              nodeCount={p.node_count}
              checked={librarySel.has(`${p.scope}-${p.id}`)}
              onToggleSelect={(e) => {
                const selId = `${p.scope}-${p.id}`;
                if (e.shiftKey) selectRange("library", selId, libVisibleIds);
                else toggleSel("library", selId);
              }}
              // A pipeline counts as "starred" when a library entry exists with
              // the same name. This is the visible link the user expects when
              // they click the canvas star: their pipeline gets a star badge
              // here, confirming the action had effect.
              starred={isStarred(p, libraryPipelines)}
              selected={p.id === activeTabId}
              // #273: scope:"library" rows now appear here in block 1 (the
              // /pipelines scope-merge from #216 means they no longer fall
              // through to the library-only block below). Surface the same Copy
              // affordance #224 shipped, gated on identity (scope), not the
              // name-absence filter that block 2 uses. `p.id` is the HOME
              // library file-stem — duplicateLibraryPipeline resolves it.
              showDuplicate={p.scope === "library"}
              onOpen={() => openPipeline(p.id, p.scope)}
              onDuplicate={() => handleDuplicate(p.id)}
              // Confirm-gated, because this row is a working pipeline file and
              // the delete may cascade to its Library twin (#227).
              onDelete={() => setDeleteTarget(p)}
              deleteTitle="Delete pipeline"
            />
          ))}
          {/* Library-only entries (no matching name in /pipelines). These
              previously only showed up in the New Run dropdown — surfacing
              them here means starring a brand-new pipeline yields a visible
              entry in the sidebar, matching the user's mental model that
              starred == in the library. No `onOpen`: there is no working
              pipeline behind them to open. */}
          {libraryOnlyEntries.map((lp) => (
            <LibraryRow
              key={`lib-only-${lp.scope}-${lp.id}`}
              name={lp.name}
              scope={lp.scope}
              nodeCount={lp.node_count}
              checked={librarySel.has(`lib-only-${lp.scope}-${lp.id}`)}
              onToggleSelect={(e) => {
                const selId = `lib-only-${lp.scope}-${lp.id}`;
                if (e.shiftKey) selectRange("library", selId, libVisibleIds);
                else toggleSel("library", selId);
              }}
              // Unconditional: these rows come straight out of the library.
              starred
              showDuplicate
              onDuplicate={() => handleDuplicate(lp.id)}
              // Direct, no confirm modal: nothing cascades from a row that only
              // exists in the library (#227 d).
              onDelete={async () => {
                try {
                  await deleteLibraryPipeline(lp.id);
                  onLibraryPipelinesChanged();
                } catch { /* ignore */ }
              }}
              deleteTitle="Remove from library"
              testId="library-only-entry"
            />
          ))}
        </div>

        {/* #577 — floating bulk-action bar for the Library tab. */}
        {librarySelIds.length > 0 && (
          <BulkActionBar
            count={librarySelIds.length}
            actions={[
              {
                key: "delete",
                label: "Delete",
                icon: <Trash2 size={13} />,
                destructive: true,
                onClick: () => setLibBulkKind("delete"),
              },
              {
                key: "duplicate",
                label: "Duplicate",
                icon: <Copy size={13} />,
                disabled: libDupItems.length === 0,
                onClick: () => setLibBulkKind("duplicate"),
              },
            ]}
            onClear={() => clearSel("library")}
          />
        )}
        {libBulkKind === "delete" && (
          <BulkActionModal
            destructive
            runningLabel="Deleting"
            title={`Delete ${libDeleteItems.length} pipeline${libDeleteItems.length === 1 ? "" : "s"}?`}
            description="This permanently removes the selected pipelines' files (YAML + prompts) from disk. This can't be undone."
            confirmLabel="Delete"
            items={libDeleteItems}
            run={(selId) => libTargetById.get(selId)!.del()}
            onClose={() => setLibBulkKind(null)}
            onSettled={handleLibrarySettled}
          />
        )}
        {libBulkKind === "duplicate" && (
          <BulkActionModal
            skipConfirm
            runningLabel="Duplicating"
            title=""
            description=""
            confirmLabel="Duplicate"
            items={libDupItems}
            run={(selId) => libTargetById.get(selId)!.dup!()}
            onClose={() => setLibBulkKind(null)}
            onSettled={handleLibrarySettled}
          />
        )}
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

      {projectEditor && (
        <ProjectEditModal
          initialProject={projectEditor.project}
          initialName={projectEditor.name}
          initialMemberPaths={projectEditor.memberPaths}
          availableRepos={availableRepos}
          projects={projects}
          onClose={() => setProjectEditor(null)}
          onSaved={() => onProjectsChanged?.()}
        />
      )}

      {confirmRetryAll && (
        <RetryAllConfirmModal
          onConfirm={() => handleRetryAll(confirmRetryAll)}
          onCancel={() => setConfirmRetryAll(null)}
        />
      )}

      {/* Show the cascade checkbox only when the target has exactly one same-name
          Library copy and isn't itself the library row (#227) — the SAME
          `cascadableTwin` call `handleConfirmDelete` acts on, so the offer and
          the deed can never disagree. */}
      <ConfirmDeleteModal
        // Remount per target so the checkbox resets to OFF each open (#227).
        key={deleteTarget?.id ?? "none"}
        open={deleteTarget !== null}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleConfirmDelete}
        name={deleteTarget?.name ?? ""}
        cascadeLabel={
          cascadableTwin(deleteTarget, libraryPipelines)
            ? "Also remove the Library copy"
            : undefined
        }
      />

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
export function RetryAllConfirmModal({
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
