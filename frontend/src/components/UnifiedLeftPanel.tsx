import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Pause, Pencil, Play, Plus, RotateCcw, SquareTerminal, Trash2, Zap } from "lucide-react";
import { isLiveRun, isTerminalRun } from "../run/model/runStatus";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import type { RunStatus } from "../shared/generated/RunStatus";
import type { Trigger } from "../shared/generated/Trigger";
import type { TriggerListEntry } from "../shared/generated/TriggerListEntry";
import type { Project } from "../workspace/model/project";
import type { LibraryPipelineEntry } from "../pipeline/api/libraryPipelines";
import { forgetRun } from "../api";
import { cleanupRun, pauseRun, renameRun, resumeRun, retryAll } from "../run/api/commands";
import { useRunShell } from "../session/store/useRunShell";
import { useEditStore } from "../pipeline/store/editStore";
import { useSelectionStore } from "../stores/selectionStore";
import { useRecentReposStore } from "../stores/recentReposStore";
import { handleSelectionKeydown } from "../lib/selectionKeys";
import { type BulkItem, type BulkOutcome } from "../lib/bulk";
import { groupByProject, type ProjectRef } from "../lib/groupByRepo";
import {
  ancestorIds, buildRunTree, cascadeImpact, filterRunTree, flattenAll, flattenVisible, parentIds,
  rootOf, type RunTreeNode,
} from "../lib/runTree";
import { loadChildRunsExpanded } from "../lib/uiPrefs";
import { projectLookup } from "../workspace/model/projectLookup";
import BulkActionBar from "./BulkActionBar";
import BulkActionModal from "./BulkActionModal";
import CleanupConfirmModal from "./CleanupConfirmModal";
import ForgetRunModal from "./ForgetRunModal";
import ProjectEditModal from "../workspace/components/ProjectEditModal";
import RunFilters from "./RunFilters";
import { EMPTY_RUN_FILTER, isFilterActive, runMatchesFilter } from "./runFilter";
import { ChildCountPills } from "./OrchestrationTab";
import RunShellModal from "../session/components/RunShellModal";
import SelectControl from "./SelectControl";
import TriggersListPanel from "./TriggersListPanel";
import PipelinesPane from "../pipeline/components/PipelinesPane";

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
  /** @deprecated Instance pipelines no longer have a library scope. */
  libraryPipelines?: LibraryPipelineEntry[];
  /** @deprecated Instance pipelines refresh through loadPipelines. */
  onLibraryPipelinesChanged?: () => void;
  /** Triggers (#160). Optional so existing callers/tests keep working. */
  triggers?: TriggerListEntry[];
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

/**
 * Title of the bulk cleanup confirm (#815): the selected runs plus the finished
 * child runs the daemon's cascade archives with them.
 */
function cleanupTitle(selected: number, finishedChildren: number): string {
  const runs = `${selected} run${selected === 1 ? "" : "s"}`;
  if (finishedChildren === 0) return `Cleanup ${runs}?`;
  return `Cleanup ${runs} and ${finishedChildren} finished child run${finishedChildren === 1 ? "" : "s"}?`;
}

export default function UnifiedLeftPanel({
  runs,
  selectedRunId,
  onSelectRun,
  onNewRun,
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
  const runShell = useRunShell();
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

  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const recentRepos = useRecentReposStore((s) => s.recentRepos);

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
  const [runBulkKind, setRunBulkKind] = useState<"cleanup" | "retry" | "pause" | null>(null);

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

  // One run row, rendered identically whether the list is flat or grouped by
  // repo (#258). Extracted so both code paths share the exact same markup.
  // #783 — the row is a TREE node: indented 14px per level, a chevron under the
  // status dot when it has children, the aggregated child counts bottom-right.
  // The tree itself (structure, counts, filter) is computed by `lib/runTree.ts`;
  // this only renders.
  function renderRunRow(node: RunTreeNode) {
    const run = node.run;
    const hasChildren = node.children.length > 0;
    const expanded = hasChildren && isExpanded(run.run_id);
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
        data-run-row={run.run_id}
        data-depth={node.depth}
        aria-level={node.depth + 1}
        aria-expanded={hasChildren ? expanded : undefined}
        onClick={() => onSelectRun(run.run_id)}
        onKeyDown={(e) => {
          // #783 decision 2 — ← / → drive the tree from the focused row: → opens
          // a collapsed parent; ← closes an open one, or climbs to the parent
          // row when there is nothing left to close.
          if (e.key === "ArrowRight" && hasChildren && !expanded) {
            e.preventDefault();
            setExpanded(run.run_id, true);
          } else if (e.key === "ArrowLeft") {
            if (hasChildren && expanded) {
              e.preventDefault();
              setExpanded(run.run_id, false);
            } else if (run.parent_run_id) {
              e.preventDefault();
              runsScrollRef.current
                ?.querySelector<HTMLElement>(`[data-run-row="${run.parent_run_id}"]`)
                ?.focus();
            }
          }
        }}
        style={{ fontSize: "11.5px", paddingLeft: 12 + node.depth * 14 }}
        className={`group flex w-full cursor-pointer items-stretch gap-2 border-b border-l-2 border-line-soft py-2 pr-3 text-left transition-colors ${
          rowSelected
            ? "border-l-acc bg-acc-bg text-fg"
            : isSelected
              ? "border-l-transparent bg-bg-3 text-fg"
              : "border-l-transparent text-fg-2 hover:bg-bg-3/50"
        } ${isArchived ? "opacity-60" : ""}`}
      >
        {/* Left column (16px, shared shape with Library / Triggers rows): the
            #577 select control, and — only on a row with children — the #783
            chevron stacked under it. Two distinct targets: the chevron never
            selects nor opens the run. */}
        <div className="flex w-4 shrink-0 flex-col items-center gap-1">
        {/* #577 — the status dot doubles as the select control: it goes hollow on
            hover and becomes a green check when selected. #503: the resting dot
            still carries the failure reason so a red Run has something to say;
            #588: an amber dot carries the awaiting reason (the agent's question,
            or « child run <name> is awaiting you »). */}
        <SelectControl
          selected={rowSelected}
          dotClass={dot}
          ringClass={ring}
          pulse={run.status === "running" && !run.stalled}
          dotTitle={run.failure_reason ?? run.awaiting_reason ?? undefined}
          dotTestId="run-status-dot"
          label={rowSelected ? `Deselect ${run.name || run.run_id}` : `Select ${run.name || run.run_id}`}
          onSelect={(e) => {
            if (e.shiftKey) selectRange("runs", run.run_id, visibleRunIds);
            else toggleSel("runs", run.run_id);
          }}
        />
        {hasChildren && (
          <span
            role="button"
            aria-label={expanded ? "Collapse child runs" : "Expand child runs"}
            aria-expanded={expanded}
            title={expanded ? "Collapse child runs" : "Expand child runs"}
            data-testid="run-tree-chevron"
            className="grid h-3.5 w-4 cursor-pointer place-items-center rounded text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2"
            onClick={(e) => {
              e.stopPropagation();
              setExpanded(run.run_id, !expanded);
            }}
          >
            {expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          </span>
        )}
        </div>
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
            {/* Provenance badge, icon-only (#725, user decision 2026-09-06): the
                words live in the `title`; `aria-label` carries the word for AT.
                #783 dropped the « orchestrated » badge — the indentation says
                the filiation; the trigger badge stays. */}
            {run.triggered_by && (
              <span
                role="button"
                aria-label="trigger"
                title={`Created by trigger “${
                  triggers.find((t) => t.id === run.triggered_by)?.name ?? run.triggered_by
                }” — click to open it in the Triggers tab`}
                className="flex shrink-0 cursor-pointer items-center rounded border border-acc px-1.5 py-[2px] text-acc transition-colors hover:bg-acc-bg"
                data-testid="run-trigger-badge"
                onClick={(e) => {
                  e.stopPropagation();
                  if (run.triggered_by) onSelectTrigger?.(run.triggered_by);
                  setActiveTab("triggers");
                }}
              >
                <Zap size={9} />
              </span>
            )}
          </div>
        </div>
        {/* Right column (#783): line 1 = the hover-revealed action icons, line 2
            = the aggregated child counts, always visible, bottom-right. The
            actions row keeps its height at rest so the pills never move when the
            icons appear. */}
        <div className="flex shrink-0 flex-col items-end justify-between gap-1">
        <div className="flex h-4 items-center">
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
            onClick={(e) => {
              e.stopPropagation();
              // Silent on failure, like handleCleanup: nothing actionable in the row.
              void runShell.open(run.run_id);
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
        </div>
        {hasChildren && (
          <ChildCountPills
            counts={node.counts}
            size="xs"
            testId="run-child-pills"
            // Decision 1 — the pills of a COLLAPSED parent expand it; once open
            // they are inert and the click reaches the row as usual.
            onClick={expanded ? undefined : () => setExpanded(run.run_id, true)}
            clickHint="click to expand"
          />
        )}
        </div>
      </button>
    );
  }

  // #336 — client-side run filters (project / pipeline / trigger), AND
  // semantics, session-only state. #783: applied to the TREE (a non-matching
  // parent is kept as a path to a matching descendant) and BEFORE the
  // active/archived split, so grouping, the Archived section and its count all
  // see the same filtered view.
  const [runFilter, setRunFilter] = useState(EMPTY_RUN_FILTER);
  const filterActive = isFilterActive(runFilter);

  // #783 — the run tree, built from ALL runs so a child follows its parent into
  // whatever section / group the ROOT lands in, whatever the child's own status
  // or repo. The archived split is on the root — and an archived run is always
  // a leaf (`buildRunTree` releases its children as roots, ADR-0064), so the
  // Archived section only ever holds archived runs, never a live child.
  const tree = useMemo(() => buildRunTree(runs), [runs]);
  const { activeTree, archivedTree } = useMemo(() => {
    const match = (r: RunListEntry) => runMatchesFilter(r, runFilter);
    return {
      activeTree: filterRunTree(tree.filter((n) => n.run.status !== "archived"), match),
      archivedTree: filterRunTree(tree.filter((n) => n.run.status === "archived"), match),
    };
  }, [tree, runFilter]);
  const filteredRuns = useMemo(
    () => [...flattenAll(activeTree), ...flattenAll(archivedTree)],
    [activeTree, archivedTree],
  );

  // #783 — expand / collapse state. `sessionDefault` is seeded ONCE from the
  // per-browser preference (Settings › Interface); per-row toggles land in
  // `expandOverrides`, the global button rewrites both. Nothing here is
  // persisted (« bascules par ligne non mémorisées »). While a filter is active
  // a kept parent opens on its matching children unless the user closed it.
  const [sessionDefault, setSessionDefault] = useState(loadChildRunsExpanded);
  const [expandOverrides, setExpandOverrides] = useState<ReadonlyMap<string, boolean>>(
    () => new Map(),
  );
  const isExpanded = (runId: string): boolean =>
    expandOverrides.get(runId) ?? (filterActive ? true : sessionDefault);
  const setExpanded = (runId: string, v: boolean) =>
    setExpandOverrides((m) => new Map(m).set(runId, v));
  const allParentIds = useMemo(() => parentIds(tree), [tree]);

  // #725/#783 — reveal the selected run: when the selection CHANGES to a run
  // whose ancestors are collapsed (Orchestration tab, URL, back navigation),
  // open them and scroll the row into view once it exists. Adjusting state
  // during render on a tracked-key change is React's reset-on-prop pattern
  // (cf. `prevSelectedArchivedId` below). The request rides STATE so the row
  // mapper stays ref-free; the effect scrolls once per new request (the value
  // changes with every selection change, so no reset is needed).
  const runsScrollRef = useRef<HTMLDivElement>(null);
  const [scrollRequest, setScrollRequest] = useState<string | null>(null);
  const [prevSelectedRunId, setPrevSelectedRunId] = useState(selectedRunId);
  if (prevSelectedRunId !== selectedRunId) {
    setPrevSelectedRunId(selectedRunId);
    if (selectedRunId !== null) {
      const chain = ancestorIds(runs, selectedRunId);
      if (chain.some((id) => !isExpanded(id))) {
        setExpandOverrides((m) => {
          const next = new Map(m);
          for (const id of chain) next.set(id, true);
          return next;
        });
      }
      if (chain.length > 0) setScrollRequest(selectedRunId);
    }
  }
  useEffect(() => {
    if (scrollRequest === null || selectedRunId !== scrollRequest) return;
    runsScrollRef.current
      ?.querySelector(`[data-run-row="${scrollRequest}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [scrollRequest, selectedRunId]);

  // #136 — archived runs live in their own flat, collapsible section below the
  // active list; the active list keeps the #258 per-repo grouping. #783: the
  // section holds archived roots (always leaves — an archived parent releases
  // its children, which climb back to the active section as roots).
  const archivedCount = archivedTree.length;

  // Identity of the selected run *iff* its tree currently sits in the archived
  // set (its root is archived — i.e. it IS archived, since an archived run
  // retains no children), else null — the signal that must reveal the section.
  const selectedArchivedId =
    selectedRunId != null && rootOf(runs, selectedRunId)?.status === "archived"
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

  // #783 — the expand/collapse-all button's state, derived from the VISIBLE
  // parents (the archived ones only while the section is open). Neutral: this
  // is not a filter and never summons the clear ✕.
  const visibleParentIds = [
    ...parentIds(activeTree),
    ...(archivedOpen ? parentIds(archivedTree) : []),
  ];
  const allExpanded =
    visibleParentIds.length > 0 ? visibleParentIds.every(isExpanded) : sessionDefault;
  const setAllExpanded = (v: boolean) => {
    setSessionDefault(v);
    // Under an active filter the default is "open", so collapsing must pin
    // every visible parent shut explicitly; expanding just drops the pins.
    setExpandOverrides(
      !v && filterActive ? new Map(visibleParentIds.map((id) => [id, false])) : new Map(),
    );
  };

  // #552 — verbatim `path → Projet` lookup, and the candidate repos the pencil
  // can attach (the distinct effective repos across ALL runs, so a filtered view
  // never hides an attachable repo).
  const projectOf = useMemo<(path: string) => ProjectRef | null>(
    () => projectLookup(projects),
    [projects],
  );
  const availableRepos = useMemo(() => {
    const set = new Set<string>();
    for (const path of recentRepos) set.add(path);
    for (const r of runs) if (r.effective_repo) set.add(r.effective_repo);
    for (const project of projects) {
      for (const path of project.members) set.add(path);
    }
    return [...set];
  }, [projects, recentRepos, runs]);

  // Group the active Runs list by Projet (#552), falling back to the #258
  // per-path grouping when nothing is named; `null` ⇒ the flat list. #783: the
  // grouping key is the ROOT's repo — a child follows its parent whatever its
  // own repo.
  const runGroups = groupByProject(activeTree, (n) => n.run.effective_repo, projectOf);
  const orderedActive: RunTreeNode[] =
    runGroups === null ? activeTree : runGroups.flatMap((g) => g.items);

  // #577 — Runs multi-select derivations. `visibleRunIds` is the flattened visible
  // order (grouped active list, then the archived section only when expanded),
  // collapsed subtrees excluded (#783) — the basis for a shift-range and for
  // select-all-visible.
  const visibleRunIds = [
    ...flattenVisible(orderedActive, isExpanded).map((n) => n.run.run_id),
    ...(archivedOpen ? flattenVisible(archivedTree, isExpanded).map((n) => n.run.run_id) : []),
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
  // #815 — the daemon's cleanup cascades to terminated descendants and refuses
  // a parent whose subtree still has a live run; the dialog says so upfront.
  const cleanupCascade = cascadeImpact(runs, cleanupItems.map((i) => i.id));
  const handleRunsSettled = useCallback(
    (o: BulkOutcome) => deselect("runs", o.succeeded.map((r) => r.id)),
    [deselect],
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
    { id: "library", label: "Pipelines" },
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
            // #823: a tour has to be able to aim at a tab — its first step opens
            // the one the rest of the tour lives in.
            data-testid={`left-tab-${tab.id}`}
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
                className="rounded-full bg-acc px-1.5 font-semibold text-on-acc"
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
            {availableRepos.length > 0 && (
              <button
                type="button"
                aria-label="Configure project"
                onClick={() => {
                  if (availableRepos.length > 1) {
                    setProjectEditor({
                      project: null,
                      name: "Project",
                      memberPaths: [],
                    });
                    return;
                  }
                  const path = availableRepos[0];
                  const project = projectOf(path);
                  openProjectEditor(
                    project
                      ? {
                          kind: "project",
                          key: `project:${project.id}`,
                          repoPath: "",
                          label: project.name,
                        }
                      : {
                          kind: "path",
                          key: path,
                          repoPath: path,
                          label: path.split("/").filter(Boolean).pop() ?? path,
                        },
                  );
                }}
                className="ml-auto cursor-pointer rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3 hover:border-acc hover:text-fg"
                style={{ fontSize: "10.5px" }}
              >
                Project
              </button>
            )}
            <button
              onClick={onNewRun}
              data-testid="new-run-button"
              className={`${availableRepos.length > 0 ? "ml-1.5" : "ml-auto"} flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim`}
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
            treeToggle={
              allParentIds.length > 0
                ? { allExpanded, onToggle: () => setAllExpanded(!allExpanded) }
                : null
            }
          />
        )}
        <div
          ref={runsScrollRef}
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
            ? flattenVisible(activeTree, isExpanded).map(renderRunRow)
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
                        flattenVisible(group.items, isExpanded).every((n) => runSel.has(n.run.run_id))
                      }
                      label={`Select all in ${group.label}`}
                      onSelect={() =>
                        selectGroup(
                          "runs",
                          flattenVisible(group.items, isExpanded).map((n) => n.run.run_id),
                        )
                      }
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
                  {flattenVisible(group.items, isExpanded).map(renderRunRow)}
                </div>
              ))}
          {/* #136 — archived runs in their own flat, collapsible section below
              the active list. Reuses renderRunRow verbatim (same archived
              styling / Forget action). The rendered gate is `archivedOpen`
              ALONE — never `archivedOpen || some(selected)`, which dead-locks
              the chevron while a selected run is archived (see decision 4). */}
          {archivedCount > 0 && (
            <div data-testid="run-archived-section" className="border-t border-line">
              <button
                onClick={() => setArchivedOpen((o) => !o)}
                className="flex w-full items-center gap-1.5 px-3 py-2 text-fg-2 transition-colors hover:bg-bg-3 cursor-pointer"
                style={{ fontSize: "11.5px" }}
                data-testid="run-archived-toggle"
              >
                {archivedOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                <span className="font-medium">
                  Archived <span data-testid="run-archived-count">({archivedCount})</span>
                </span>
              </button>
              {archivedOpen && flattenVisible(archivedTree, isExpanded).map(renderRunRow)}
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
            title={cleanupTitle(cleanupItems.length, cleanupCascade.finished)}
            description={
              <>
                This removes the selected runs' worktrees from disk and archives them.
                {runningWillStop > 0
                  ? ` ${runningWillStop} running run${runningWillStop === 1 ? "" : "s"} will be stopped first.`
                  : ""}
                {cleanupCascade.live > 0
                  ? ` ${cleanupCascade.live} child run${cleanupCascade.live === 1 ? " is" : "s are"} still live: the daemon will refuse to archive ${cleanupCascade.live === 1 ? "its" : "their"} parent until ${cleanupCascade.live === 1 ? "it is" : "they are"} stopped.`
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
      {activeTab === "library" && <PipelinesPane />}

      {confirmCleanup && (
        <CleanupConfirmModal
          runId={confirmCleanup.runId}
          isLive={
            isLiveRun(confirmCleanup.status)
          }
          cascade={cascadeImpact(runs, [confirmCleanup.runId])}
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

      {runShell.shell && (
        <RunShellModal session={runShell.shell.session} onClose={runShell.close} />
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
    </aside>
  );
}

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
