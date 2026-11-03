import { useMemo, type CSSProperties } from "react";
import { ChevronDown, ChevronRight, Pause, Play, RotateCcw, Trash2 } from "lucide-react";
import { waitingForStartedNodesLabel } from "../../model/nodePlay";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import type { RunTreeNode } from "../../model/runTree";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../../../shared/extensions/contract";
import { StatusIcon } from "../../../shared/ui/kit";
import StatusMark from "../../../shared/ui/leftList/StatusMark";
import HoverBar, { RowAction } from "../../../shared/ui/leftList/HoverBar";
import RunRowMenu, { type RunRowMenuPane } from "./RunRowMenu";
import RunParentLink, { type RunParentLinkPane } from "./RunParentLink";
import RunRowPeople from "./RunRowPeople";
import { ChildPills, NodePips, RunFigures, TriggerMark } from "./RunRowParts";
import { shownNodes } from "../../model/runRowNodes";
import type { RunGesture } from "../../model/runGestures";
import { hasSecondLine, isSecondLineElement, RUN_ROW, RUN_ROW_LOCATIONS } from "../../model/runRowElements";
import { useAppearancePreview, useElementMarks } from "../../../hooks/useElementMarks";
import { useAppearance } from "../../../hooks/useAppearance";
import { isVisible } from "../../../lib/appearance";
import { useHideableContributions } from "../../../shared/extensions/hideableContributions";

const RUN_STATUS_WORD: Record<RunListEntry["status"] | "stalled", string> = {
  running: "Running",
  awaiting_user: "Awaiting you",
  completed: "Completed",
  failed: "Failed",
  skipped: "Skipped",
  halted: "Halted",
  paused: "Paused",
  archived: "Archived",
  stalled: "Stalled",
};

/** What a row reads of the Runs tab's state. */
export type RunRowPane = Pick<
  RunsPaneState,
  | "isExpanded"
  | "setExpanded"
  | "runSel"
  | "renamingRunId"
  | "renameValue"
  | "setRenameValue"
  | "renameInputRef"
  | "commitRename"
  | "cancelRename"
  | "startRename"
  | "handlePause"
  | "handleResume"
  | "setConfirmRetryAll"
  | "setConfirmForget"
  | "runsScrollRef"
  | "selectRange"
  | "toggleSel"
  | "visibleRunIds"
  | "runById"
  | "mayOnRow"
> &
  RunParentLinkPane &
  RunRowMenuPane;

interface Props {
  node: RunTreeNode;
  pane: RunRowPane;
  selectedRunId: string | null;
  triggers: TriggerListEntry[];
  onSelectRun: (runId: string) => void;
  onSelectTrigger?: (triggerId: string) => void;
  /** Switch the left panel to its Triggers tab (the trigger mark's target). */
  onShowTriggers: () => void;
  /** A sample Run's own cost, in a preview of Settings › Appearance. */
  sampleCost?: number;
}

/**
 * One Run of the Runs list, on two lines (#258, #783): its status
 * (the select control too), its name, its Trigger, its child counts and its
 * nodes' progress; then its pipeline, the extensions' marks and people, its
 * duration and its cost. Indented 12 px per level of the tree, which
 * `model/runTree.ts` computes; this only renders. The hover bar holds the one
 * action its status calls for and the « ⋯ » menu.
 *
 * Every element but its status and its name is an Élément masquable
 * (`model/runRowElements.ts`): one the Apparence hides is not rendered, and the
 * row takes one line when its whole second line is hidden — the Apparence
 * decides, not the Run, so every row of the list keeps one height. In a preview
 * of Settings › Appearance, pointing to an element of a hidden second line
 * brings that line back as a ghost (`data-ghost-line`), the row still counting
 * one line (`data-lines="1"`). A preview's row is inert too: no hover bar, out
 * of the tab order.
 */
export default function RunRow({
  node,
  pane,
  selectedRunId,
  triggers,
  onSelectRun,
  onSelectTrigger,
  onShowTriggers,
  sampleCost,
}: Props) {
  const {
    isExpanded, setExpanded, runSel, renamingRunId, renameValue, setRenameValue, renameInputRef,
    commitRename, cancelRename, startRename, handlePause, handleResume, setConfirmRetryAll,
    setConfirmForget, runsScrollRef, selectRange, toggleSel, visibleRunIds, runById, mayOnRow,
  } = pane;
  const run = node.run;
  const preview = useAppearancePreview();
  const marksOf = useElementMarks();
  const contributions = useHideableContributions("runs.list", RUN_ROW_LOCATIONS);
  // From what the Apparence shows alone: a preview's ghost never keeps a line.
  const appearance = useAppearance();
  const contributionIds = contributions.map((c) => c.id);
  const secondLine = hasSecondLine((id) => isVisible(appearance, id), contributionIds);
  // A root whose parent sits in the other section (active / archived) links to it.
  const linkedParent = node.sectionParentLink ? runById.get(node.sectionParentLink) : undefined;
  // A preview points to an element of a hidden second line that this row can
  // draw: the line comes back as a ghost, to show that element at its place;
  // the row still counts one line.
  const pointed = preview?.pointed ?? null;
  const ghostLine =
    !secondLine &&
    pointed !== null &&
    isSecondLineElement(pointed, contributionIds) &&
    (pointed !== RUN_ROW.parent || linkedParent !== undefined) &&
    !(preview?.sample && contributionIds.includes(pointed));
  const triggerMarks = marksOf(RUN_ROW.trigger);
  const pillsMarks = marksOf(RUN_ROW.childPills);
  const pipsMarks = marksOf(RUN_ROW.nodePips);
  const pipelineMarks = marksOf(RUN_ROW.pipeline);
  const parentMarks = marksOf(RUN_ROW.parent);
  // ADR-0081 (#31): what the `run/item` decorations mark.
  const decorated = useMemo(
    () => ({
      entity: {
        kind: "run",
        id: run.run_id,
        attributes: run.attributes ?? {},
        ...(run.name ? { name: run.name } : {}),
      },
    }),
    [run.run_id, run.attributes, run.name],
  );
  const hasChildren = node.children.length > 0;
  const expanded = hasChildren && isExpanded(run.run_id);
  const isOpen = run.run_id === selectedRunId;
  const rowSelected = runSel.has(run.run_id);
  const isArchived = run.status === "archived";
  // CORE-42: each gesture also needs the caller's right to make it.
  const may = (gesture: RunGesture) => mayOnRow(run.run_id, gesture);
  const canCleanup = !isArchived && may("cleanup_run");
  const canRename = may("rename_run");
  const canForget = isArchived && may("delete");
  const isRenaming = renamingRunId === run.run_id;
  const label = run.name || run.run_id.slice(0, 20);
  // #110 — gate on EXPLICIT statuses, never isLiveRun/isTerminalRun: the former
  // includes `paused` (would wrongly show Pause on a paused run → 409); the
  // latter includes `archived` (would wrongly show Retry-all on archived → 409).
  // Each set is a subset of its daemon guard's accepted statuses.
  const canPause = (run.status === "running" || run.status === "awaiting_user") && may("pause_run");
  const canResume = run.status === "paused" && may("resume_run");
  const canRetryAll =
    (run.status === "completed" ||
      run.status === "failed" ||
      run.status === "halted" ||
      run.status === "skipped") &&
    may("retry_all");
  // A stalled run (no node running/waiting, nothing schedulable; #180) shows
  // its own steady mark over its still-`running` canonical status — "never a
  // silent stall". `stalled` is derived per read by the daemon.
  // A status this front does not know yet (a newer daemon) reads as running.
  const shownStatus = run.stalled ? "stalled" : run.status in RUN_STATUS_WORD ? run.status : "running";
  // #503: the mark carries the failure reason; #588: the awaiting reason.
  const reason =
    run.failure_reason ??
    run.awaiting_reason ??
    (run.waiting_for_started_nodes
      ? `Reached End, ${waitingForStartedNodesLabel(run.waiting_for_started_nodes)}`
      : undefined);
  const trigger = run.triggered_by
    ? (triggers.find((t) => t.id === run.triggered_by)?.name ?? run.triggered_by)
    : null;

  return (
    <div
      role="button"
      tabIndex={preview ? -1 : 0}
      className="lp-row"
      data-run-row={run.run_id}
      data-lines={secondLine ? 2 : 1}
      data-ghost-line={ghostLine || undefined}
      data-depth={node.depth}
      data-open={isOpen || undefined}
      data-checked={rowSelected || undefined}
      data-archived={isArchived || undefined}
      // Its name and its status: the rest of the row has its own names.
      aria-label={`${label}, ${RUN_STATUS_WORD[shownStatus]}`}
      aria-current={isOpen || undefined}
      aria-level={node.depth + 1}
      aria-expanded={hasChildren ? expanded : undefined}
      style={{ "--depth": node.depth } as CSSProperties}
      onClick={() => onSelectRun(run.run_id)}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelectRun(run.run_id);
        } else if (e.key === "F2" && canRename) {
          e.preventDefault();
          startRename(run);
        } else if (e.key === "ArrowRight" && hasChildren && !expanded) {
          // #783 decision 2 — ← / → drive the tree from the focused row: → opens
          // a collapsed parent; ← closes an open one, or climbs to the parent
          // row when there is nothing left to close.
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
    >
      <span className="lp-lead">
        {hasChildren ? (
          <button
            type="button"
            className="lp-chev"
            aria-label={expanded ? "Collapse child runs" : "Expand child runs"}
            title={expanded ? "Collapse child runs" : "Expand child runs"}
            aria-expanded={expanded}
            data-testid="run-tree-chevron"
            onClick={(e) => {
              e.stopPropagation();
              setExpanded(run.run_id, !expanded);
            }}
            onKeyDown={(e) => e.stopPropagation()}
          >
            {expanded ? <ChevronDown size={14} strokeWidth={1.5} /> : <ChevronRight size={14} strokeWidth={1.5} />}
          </button>
        ) : (
          <span className="lp-chev lp-chev--none" />
        )}
        {/* #577 — the status mark doubles as the select control. */}
        <StatusMark
          glyph={
            <span data-testid="run-status-dot" data-status={shownStatus}>
              <StatusIcon kind="run" status={shownStatus} title={reason} />
            </span>
          }
          selected={rowSelected}
          selecting={runSel.size > 0}
          label={rowSelected ? `Deselect ${label}` : `Select ${label}`}
          title={`${RUN_STATUS_WORD[shownStatus]}${reason ? `: ${reason}` : ""}\nClick to select, Shift+click for a range`}
          testId="run-select-control"
          onSelect={(e) => {
            if (e.shiftKey) selectRange("runs", run.run_id, visibleRunIds);
            else toggleSel("runs", run.run_id);
          }}
        />
      </span>
      <span className="lp-l1">
        {isRenaming ? (
          <input
            ref={renameInputRef}
            className="lp-rename"
            aria-label="Name"
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onBlur={() => commitRename()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") commitRename();
              if (e.key === "Escape") cancelRename();
            }}
            onClick={(e) => e.stopPropagation()}
            data-testid="rename-input"
          />
        ) : (
          <span className="lp-name" title={run.name || run.run_id} data-testid="run-display-label">
            {label}
          </span>
        )}
        {trigger !== null && triggerMarks && (
          <TriggerMark
            name={trigger}
            marks={triggerMarks}
            onOpen={() => {
              if (run.triggered_by) onSelectTrigger?.(run.triggered_by);
              onShowTriggers();
            }}
          />
        )}
        {hasChildren && pillsMarks && (
          <ChildPills
            counts={node.counts}
            marks={pillsMarks}
            // Decision 1 — the pills of a COLLAPSED parent expand it; once open
            // they are inert and the click reaches the row as usual.
            onExpand={expanded ? undefined : () => setExpanded(run.run_id, true)}
          />
        )}
        <span className="lp-end">
          {pipsMarks && <NodePips nodes={shownNodes(run)} marks={pipsMarks} />}
        </span>
      </span>
      {(secondLine || ghostLine) && (
        <span className="lp-l2">
          {pipelineMarks && (
            <span className="lp-pipe" title={run.pipeline_name} data-testid="run-pipeline-name" {...pipelineMarks}>
              {run.pipeline_name}
            </span>
          )}
          {linkedParent && parentMarks && (
            <RunParentLink parent={linkedParent} pane={pane} onSelectRun={onSelectRun} marks={parentMarks} />
          )}
          <span className="lp-end">
            {/* The extensions' marks (`run/item`, ADR-0081) and people
                (`run/item/people`, API 0.12), before the figures; nothing at all
                without a contribution, or with every one hidden. */}
            <span className="lp-deco">
              <ExtensionDecorations<DecorationContext> location="run/item" extra={decorated} nowrap />
              <RunRowPeople run={run} />
            </span>
            <RunFigures
              run={run}
              duration={marksOf(RUN_ROW.duration)}
              cost={marksOf(RUN_ROW.cost)}
              sampleCost={sampleCost}
            />
          </span>
        </span>
      )}
      {!isRenaming && !preview && (
        <HoverBar testId="run-row-actions">
          {canPause ? (
            <RowAction icon={Pause} label="Pause" testId="pause-run-button" onClick={() => handlePause(run.run_id)} />
          ) : canResume ? (
            <RowAction icon={Play} label="Resume" testId="resume-run-button" onClick={() => handleResume(run.run_id)} />
          ) : canRetryAll ? (
            <RowAction
              icon={RotateCcw}
              label="Retry all"
              testId="retry-all-button"
              onClick={() => setConfirmRetryAll(run.run_id)}
            />
          ) : canForget ? (
            <RowAction icon={Trash2} label="Forget" danger testId="forget-run-button" onClick={() => setConfirmForget(run.run_id)} />
          ) : null}
          <RunRowMenu
            run={run}
            pane={pane}
            isRenaming={isRenaming}
            canPause={canPause}
            canResume={canResume}
            canRetryAll={canRetryAll}
            canCleanup={canCleanup}
            canRename={canRename}
            canForget={canForget}
          />
        </HoverBar>
      )}
    </div>
  );
}
