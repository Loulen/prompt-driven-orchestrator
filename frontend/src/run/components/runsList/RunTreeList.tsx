import { Funnel, Play, Plus } from "lucide-react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import { handleSelectionKeydown } from "../../../lib/selectionKeys";
import { flattenAll, flattenVisible, type RunTreeNode } from "../../model/runTree";
import GroupHeader from "../../../shared/ui/leftList/GroupHeader";
import ListEmpty from "../../../shared/ui/leftList/ListEmpty";
import RunRow from "./RunRow";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../../../shared/extensions/contract";
import { whyNoRunAnywhere } from "../../model/runCreationRights";
import { useRunCreationRightsStore } from "../../store/runCreationRightsStore";

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
  /** The empty list's one action. */
  onNewRun: () => void;
}

/**
 * The scrollable run tree: the flat or Projet-grouped active list, then the
 * collapsible Archived section. Each group draws a hairline under every row
 * but its last.
 */
export default function RunTreeList({
  pane,
  runs,
  triggers,
  selectedRunId,
  onSelectRun,
  onSelectTrigger,
  onShowTriggers,
  onNewRun,
}: Props) {
  const {
    runsScrollRef, visibleRunIds, runSelIds, selectVisible, clearSel, cleanupItems, setRunBulkKind,
    filterActive, filteredRuns, clearFilters, runGroups, activeTree, isExpanded, runSel, selectGroup,
    openProjectSettings, canOpenProjectSettings, archivedCount, archivedOpen, setArchivedOpen, archivedTree,
    isGroupCollapsed, toggleGroup,
  } = pane;
  const archivedVisible = archivedOpen ? flattenVisible(archivedTree, isExpanded) : [];
  // CORE-5, CORE-205: New run greyed, with why, where no Projet allows creating a Run.
  const noRunReason = useRunCreationRightsStore((s) => whyNoRunAnywhere(s.rights));
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

  const rows = (nodes: RunTreeNode[]) => <div className="lp-rows">{nodes.map(renderRunRow)}</div>;

  return (
    <div
      ref={runsScrollRef}
      className="lp-scroll"
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
        <ListEmpty
          icon={Play}
          title="No runs"
          actionLabel="New run"
          actionIcon={Plus}
          onAction={onNewRun}
          actionRefused={noRunReason}
          testId="runs-empty"
        >
          {/* API 0.11: what the extensions say about the empty list; nothing
              at all without a contribution. */}
          <ExtensionDecorations<DecorationContext>
            location="runs/empty"
            extra={RUNS_EMPTY}
            block
            className="flex-col items-center px-3"
          />
        </ListEmpty>
      )}
      {runs.length > 0 && filterActive && filteredRuns.length === 0 && (
        <ListEmpty
          icon={Funnel}
          title="No match"
          actionLabel="Clear filters"
          onAction={clearFilters}
          testId="run-filter-empty"
        />
      )}
      {runGroups === null
        ? rows(flattenVisible(activeTree, isExpanded))
        : runGroups.map((group) => {
            const collapsed = isGroupCollapsed(group.key);
            const groupRuns = flattenAll(group.items);
            const visible = flattenVisible(group.items, isExpanded);
            const awaiting = groupRuns.filter((r) => r.status === "awaiting_user").length;
            return (
              <section
                key={group.key}
                className="lp-section"
                data-testid="run-repo-group"
                data-project={group.kind === "project" ? "true" : "false"}
                data-collapsed={collapsed ? "true" : "false"}
              >
                <GroupHeader
                  label={group.label}
                  title={group.title}
                  count={groupRuns.length}
                  deleted={group.deleted}
                  collapsed={collapsed}
                  onToggle={() => toggleGroup(group.key)}
                  awaiting={awaiting}
                  // #577 — "select all in this Projet" (on header hover).
                  allSelected={visible.length > 0 && visible.every((n) => runSel.has(n.run.run_id))}
                  onSelectAll={() => selectGroup("runs", visible.map((n) => n.run.run_id))}
                  // #552 — the pencil: Settings › Projects on this group (CORE-96).
                  onEdit={!group.deleted && canOpenProjectSettings ? () => openProjectSettings(group) : undefined}
                  editLabel={group.kind === "project" ? "Edit project" : "Name project"}
                  testPrefix="run-group"
                  labelTestId="run-repo-label"
                />
                {!collapsed && rows(visible)}
              </section>
            );
          })}
      {/* #136 — archived runs in their own collapsible section below the
          active list, rendered as a tree like the active one. The rendered
          gate is `archivedOpen` ALONE — never `archivedOpen || some(selected)`,
          which dead-locks the toggle while a selected run is archived
          (see decision 4). */}
      {archivedCount > 0 && (
        <section className="lp-section" data-testid="run-archived-section">
          <GroupHeader
            label="Archived"
            count={archivedCount}
            collapsed={!archivedOpen}
            onToggle={() => setArchivedOpen((o) => !o)}
            allSelected={
              archivedVisible.length > 0 && archivedVisible.every((n) => runSel.has(n.run.run_id))
            }
            onSelectAll={() => selectGroup("runs", archivedVisible.map((n) => n.run.run_id))}
            testPrefix="run-archived"
            labelTestId="run-archived-label"
          />
          {archivedOpen && rows(archivedVisible)}
        </section>
      )}
    </div>
  );
}
