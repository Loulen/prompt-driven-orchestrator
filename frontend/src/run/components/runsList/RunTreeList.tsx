import { ChevronDown, ChevronRight, Pencil } from "lucide-react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import { handleSelectionKeydown } from "../../../lib/selectionKeys";
import { flattenAll, flattenVisible, type RunTreeNode } from "../../model/runTree";
import { EMPTY_RUN_FILTER } from "../../model/runFilter";
import SelectControl from "../../../shared/ui/SelectControl";
import RunRow from "./RunRow";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../../../shared/extensions/contract";

/** The empty Runs list is about no entity (`runs/empty`). */
const RUNS_EMPTY: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

interface Props {
  pane: RunsPaneState;
  runs: RunListEntry[];
  triggers: TriggerListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onSelectTrigger?: (triggerId: string) => void;
  onShowTriggers: () => void;
}

/**
 * The scrollable run tree: the flat or Projet-grouped active list, then the
 * collapsible Archived section.
 */
export default function RunTreeList({
  pane,
  runs,
  triggers,
  selectedRunId,
  onSelectRun,
  onSelectTrigger,
  onShowTriggers,
}: Props) {
  const {
    runsScrollRef, visibleRunIds, runSelIds, selectVisible, clearSel, cleanupItems, setRunBulkKind,
    filterActive, filteredRuns, setRunFilter, runGroups, activeTree, isExpanded, runSel, selectGroup,
    openProjectEditor, archivedCount, archivedOpen, setArchivedOpen, archivedTree, setAttributeFilter,
    setProjectFilter, isGroupCollapsed, toggleGroup,
  } = pane;
  const renderRunRow = (node: RunTreeNode) => (
    <RunRow
      key={node.run.run_id}
      node={node}
      pane={pane}
      selectedRunId={selectedRunId}
      triggers={triggers}
      onSelectRun={onSelectRun}
      onSelectTrigger={onSelectTrigger}
      onShowTriggers={onShowTriggers}
    />
  );

  return (
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
          data-testid="runs-empty"
        >
          No runs yet
          {/* API 0.11: what the extensions say about the empty list; nothing
              at all without a contribution. */}
          <ExtensionDecorations<DecorationContext>
            location="runs/empty"
            extra={RUNS_EMPTY}
            block
            className="mt-1 flex-col"
          />
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
            onClick={() => {
              setRunFilter(EMPTY_RUN_FILTER);
              setAttributeFilter({});
              setProjectFilter({});
            }}
          >
            Clear filters
          </button>
        </div>
      )}
      {runGroups === null
        ? flattenVisible(activeTree, isExpanded).map(renderRunRow)
        : runGroups.map((group) => {
            const collapsed = isGroupCollapsed(group.key);
            const groupRuns = flattenAll(group.items);
            const awaiting = groupRuns.filter((r) => r.status === "awaiting_user").length;
            return (
            <div
              key={group.key}
              data-testid="run-repo-group"
              data-project={group.kind === "project" ? "true" : "false"}
              data-collapsed={collapsed ? "true" : "false"}
            >
              <div
                className="group group/hdr flex h-[22px] shrink-0 items-center gap-1 border-b border-line-soft bg-bg-3/40 px-3 font-medium text-fg-3"
                style={{ fontSize: "10px" }}
                title={group.title}
              >
                {/* The fold of the group, kept per browser (local storage). */}
                <button
                  type="button"
                  onClick={() => toggleGroup(group.key)}
                  aria-expanded={!collapsed}
                  aria-label={collapsed ? `Expand ${group.label}` : `Collapse ${group.label}`}
                  title={collapsed ? "Expand project" : "Collapse project"}
                  className="grid h-4 w-3 shrink-0 cursor-pointer place-items-center rounded text-fg-4 transition-colors hover:text-fg-2"
                  data-testid="run-group-chevron"
                >
                  {collapsed ? <ChevronRight size={10} /> : <ChevronDown size={10} />}
                </button>
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
                <span className="shrink-0 font-mono font-normal text-fg-4" data-testid="run-group-count">
                  {groupRuns.length}
                  {collapsed && awaiting > 0 && ` · ${awaiting} awaiting`}
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
                {collapsed && awaiting > 0 && (
                  <span
                    className="ml-auto block h-1.5 w-1.5 shrink-0 rounded-full bg-st-await group-hover/hdr:ml-1"
                    title={`${awaiting} awaiting you`}
                    data-testid="run-group-awaiting"
                  />
                )}
              </div>
              {!collapsed && flattenVisible(group.items, isExpanded).map(renderRunRow)}
            </div>
            );
          })}
      {/* #136 — archived runs in their own collapsible section below the
          active list, rendered as a tree like the active one. Reuses
          renderRunRow verbatim (same archived styling / Forget action).
          The rendered gate is `archivedOpen` ALONE — never
          `archivedOpen || some(selected)`, which dead-locks the chevron
          while a selected run is archived (see decision 4). */}
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
  );
}
