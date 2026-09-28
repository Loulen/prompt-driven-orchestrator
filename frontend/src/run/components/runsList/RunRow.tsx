import { ChevronDown, ChevronRight, Pause, Pencil, Play, RotateCcw, SquareTerminal, Trash2, Zap } from "lucide-react";
import { isLiveRun, isTerminalRun } from "../../model/runStatus";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import type { RunTreeNode } from "../../model/runTree";
import { ChildCountPills } from "../OrchestrationTab";
import SelectControl from "../../../shared/ui/SelectControl";
import { STATUS_STYLES } from "./runsListTables";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

interface Props {
  node: RunTreeNode;
  pane: RunsPaneState;
  selectedRunId: string | null;
  triggers: TriggerListEntry[];
  onSelectRun: (runId: string) => void;
  onSelectTrigger?: (triggerId: string) => void;
  /** Switch the left panel to its Triggers tab (the trigger badge's target). */
  onShowTriggers: () => void;
}

// One run row, rendered identically whether the list is flat or grouped by
// repo (#258). Extracted so both code paths share the exact same markup.
// #783 — the row is a TREE node: indented 14px per level, a chevron under the
// status dot when it has children, the aggregated child counts bottom-right.
// The tree itself (structure, counts, filter) is computed by `lib/runTree.ts`;
// this only renders.
export default function RunRow({
  node,
  pane,
  selectedRunId,
  triggers,
  onSelectRun,
  onSelectTrigger,
  onShowTriggers,
}: Props) {
  const {
    isExpanded, setExpanded, runSel, renamingRunId, renameValue, setRenameValue, renameInputRef,
    commitRename, cancelRename, startRename, handlePause, handleResume, setConfirmRetryAll,
    setConfirmCleanup, setConfirmForget, runShell, runsScrollRef, selectRange, toggleSel,
    visibleRunIds,
  } = pane;
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
                onShowTriggers();
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
