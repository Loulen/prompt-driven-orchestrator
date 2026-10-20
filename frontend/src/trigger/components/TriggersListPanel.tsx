import { useMemo, useState } from "react";
import { Pencil, Plus, Zap } from "lucide-react";
import type { Trigger } from "../../shared/generated/Trigger";
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";
import type { Project } from "../../workspace/model/project";
import { deleteTrigger, updateTrigger } from "../api/triggers";
import { groupByProject } from "../../lib/groupByRepo";
import { recordedProject } from "../../workspace/model/projectLookup";
import { useSelectionStore } from "../../stores/selectionStore";
import { handleSelectionKeydown } from "../../lib/selectionKeys";
import { projectsEntryOf, type ProjectsEntry } from "../../workspace/model/projectsSettings";
import SelectControl from "../../shared/ui/SelectControl";
import TriggerRow from "./triggersList/TriggerRow";
import TriggersBulk, { type TriggerBulkKind } from "./triggersList/TriggersBulk";
import TriggersListHeader from "./triggersList/TriggersListHeader";
import ExtensionListFilters from "../../shared/extensions/ExtensionListFilters";
import {
  isAttributeFilterActive,
  matchesAttributes,
  type Attributes,
} from "../../shared/extensions/attributes";

interface Props {
  triggers: TriggerListEntry[];
  selectedTriggerId: string | null;
  onSelectTrigger: (triggerId: string) => void;
  onNewTrigger: () => void;
  onTriggersChanged: () => void;
  /** Open the New Run modal pre-filled from this Trigger (Run-now mode). */
  onRunNow?: (trigger: Trigger) => void;
  /** Open the modal in edit mode for this Trigger. */
  onEditTrigger?: (trigger: Trigger) => void;
  /** #348 global kill-switch: whether all scheduled fires are currently paused. */
  paused?: boolean;
  /** #348: flip the global pause flag. */
  onTogglePause?: () => void;
  /** Projets (#552) — the group-by-Projet layer. Optional so existing
   *  callers/tests keep working. */
  projects?: Project[];
  /** CORE-96: the pencil of a group header opens Settings › Projects there. */
  onOpenProjectSettings?: (entry: ProjectsEntry) => void;
}

export default function TriggersListPanel({
  triggers: allTriggers,
  selectedTriggerId,
  onSelectTrigger,
  onNewTrigger,
  onTriggersChanged,
  onRunNow,
  onEditTrigger,
  paused = false,
  onTogglePause,
  projects = [],
  onOpenProjectSettings,
}: Props) {

  // #577 — multi-select state lives in the shared per-tab store; a pending bulk
  // action is local (drives the confirm/progress modal).
  const selectedIds = useSelectionStore((s) => s.triggers);
  const toggle = useSelectionStore((s) => s.toggle);
  const selectRange = useSelectionStore((s) => s.selectRange);
  const selectVisible = useSelectionStore((s) => s.selectVisible);
  const selectGroup = useSelectionStore((s) => s.selectGroup);
  const clearSel = useSelectionStore((s) => s.clear);
  const selSet = useMemo(() => new Set(selectedIds), [selectedIds]);
  const [pending, setPending] = useState<TriggerBulkKind | null>(null);

  // ADR-0081 (#31) — the extensions' list filters: `triggers/filter` on the
  // Triggers' own attributes, `projects/filter` on the attributes of the Projet
  // each Trigger's repository belongs to (the list's grouping rule).
  const [attributeFilter, setAttributeFilter] = useState<Attributes>({});
  const [projectFilter, setProjectFilter] = useState<Attributes>({});
  const filterActive =
    isAttributeFilterActive(attributeFilter) || isAttributeFilterActive(projectFilter);
  const triggers = useMemo(() => {
    if (!filterActive) return allTriggers;
    const byId = new Map(projects.map((p) => [p.id, p]));
    return allTriggers.filter((t) => {
      if (!matchesAttributes(t.attributes, attributeFilter)) return false;
      if (!isAttributeFilterActive(projectFilter)) return true;
      const project = t.project_id ? byId.get(t.project_id) : undefined;
      return project !== undefined && matchesAttributes(project.attributes, projectFilter);
    });
  }, [allTriggers, projects, attributeFilter, projectFilter, filterActive]);

  async function handleDelete(triggerId: string) {
    try {
      await deleteTrigger(triggerId);
      onTriggersChanged();
    } catch {
      // WS push / refresh will reconcile.
    }
  }

  async function handleToggle(t: Trigger) {
    try {
      await updateTrigger(t.id, { enabled: !t.enabled });
      onTriggersChanged();
    } catch {
      // WS push / refresh will reconcile.
    }
  }

  // Raw target repos actually set on rows — drives the per-row badge label so a
  // colliding basename is disambiguated identically in the badge and the group
  // header (#258 G7). Computed once per render.
  const allRepos = triggers
    .map((t) => t.target_repo)
    .filter((r): r is string => !!r);

  // Group the Triggers list by the Projet each launches in (#552, ADR-0087),
  // falling back to the #258 per-path grouping; `null` ⇒ the flat list.
  const projectOfTrigger = useMemo(() => recordedProject(projects), [projects]);
  const triggerGroups = groupByProject(
    triggers,
    (t) => t.effective_repo,
    (t) => projectOfTrigger(t.project_id),
  );

  // Visible order (post-grouping) — the basis for shift-range and select-all.
  const orderedIds =
    triggerGroups === null
      ? triggers.map((t) => t.id)
      : triggerGroups.flatMap((g) => g.items.map((t) => t.id));

  const renderTriggerRow = (t: Trigger) => (
    <TriggerRow
      key={t.id}
      trigger={t}
      isSelected={t.id === selectedTriggerId}
      rowSelected={selSet.has(t.id)}
      allRepos={allRepos}
      onSelect={() => onSelectTrigger(t.id)}
      onSelectControl={(shiftKey) => {
        if (shiftKey) selectRange("triggers", t.id, orderedIds);
        else toggle("triggers", t.id);
      }}
      onRunNow={onRunNow && (() => onRunNow(t))}
      onEdit={onEditTrigger && (() => onEditTrigger(t))}
      onDelete={() => void handleDelete(t.id)}
      onToggle={() => void handleToggle(t)}
    />
  );

  // The group-header "select all in this repo" control (#577).
  function renderGroupSelectAll(group: { items: TriggerListEntry[]; label: string }) {
    const groupIds = group.items.map((t) => t.id);
    const groupSelected = groupIds.length > 0 && groupIds.every((id) => selSet.has(id));
    return (
      <SelectControl
        selected={groupSelected}
        label={`Select all in ${group.label}`}
        onSelect={() => selectGroup("triggers", groupIds)}
        testId="trigger-group-select-all"
      />
    );
  }

  return (
    <div className="flex h-full flex-col" data-testid="triggers-list-panel">
      <TriggersListHeader
        paused={paused}
        onTogglePause={onTogglePause}
        onNewTrigger={onNewTrigger}
      />

      <ExtensionListFilters
        location="triggers/filter"
        filter={attributeFilter}
        onChange={setAttributeFilter}
      />
      <ExtensionListFilters
        location="projects/filter"
        filter={projectFilter}
        onChange={setProjectFilter}
      />

      {allTriggers.length > 0 && triggers.length === 0 ? (
        <div
          className="px-3 py-4 text-center text-fg-4"
          style={{ fontSize: "11px" }}
          data-testid="trigger-filter-empty"
        >
          No triggers match filters
          <button
            className="mt-1 block w-full cursor-pointer text-acc hover:underline"
            onClick={() => {
              setAttributeFilter({});
              setProjectFilter({});
            }}
          >
            Clear filters
          </button>
        </div>
      ) : triggers.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
          <Zap size={22} className="text-fg-4" />
          <div className="text-fg-3" style={{ fontSize: "12px" }}>
            No triggers yet
          </div>
          <div className="text-fg-4" style={{ fontSize: "10.5px", lineHeight: 1.4 }}>
            Triggers start a Run on a schedule, so recurring work happens without
            you launching it by hand.
          </div>
          <button
            onClick={onNewTrigger}
            className="mt-1 flex cursor-pointer items-center gap-1 rounded bg-acc px-2 py-1 font-medium text-on-acc transition-colors hover:bg-acc-dim"
            style={{ fontSize: "10.5px" }}
          >
            <Plus size={11} />
            New Trigger
          </button>
        </div>
      ) : (
        <div
          className="flex-1 overflow-y-auto outline-none"
          tabIndex={-1}
          onKeyDown={(e) =>
            handleSelectionKeydown(e, {
              tab: "triggers",
              visibleIds: orderedIds,
              hasSelection: selectedIds.length > 0,
              selectVisible,
              clear: clearSel,
              onBulkDelete: () => {
                if (selectedIds.length > 0) setPending("delete");
              },
            })
          }
        >
          {triggerGroups === null
            ? triggers.map(renderTriggerRow)
            : triggerGroups.map((group) => (
                <div
                  key={group.key}
                  data-testid="trigger-repo-group"
                  data-project={group.kind === "project" ? "true" : "false"}
                >
                  <div
                    className="group group/hdr flex h-[22px] shrink-0 items-center gap-1 border-b border-line-soft bg-bg-3/40 px-3 font-medium text-fg-3"
                    style={{ fontSize: "10px" }}
                    title={group.title}
                  >
                    {renderGroupSelectAll(group)}
                    <span
                      className={`truncate ${group.deleted ? "italic text-fg-4" : ""}`}
                      data-testid="trigger-repo-label"
                    >
                      {group.label}
                    </span>
                    {/* #552 — the pencil: Settings › Projects on this group (CORE-96). */}
                    {!group.deleted && onOpenProjectSettings && (
                    <button
                      onClick={() => onOpenProjectSettings(projectsEntryOf(group))}
                      className="ml-auto hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover/hdr:inline-flex"
                      title={group.kind === "project" ? "Edit project" : "Name project"}
                      aria-label={group.kind === "project" ? "Edit project" : "Name project"}
                      data-testid="trigger-group-pencil"
                    >
                      <Pencil size={10} />
                    </button>
                    )}
                  </div>
                  {group.items.map(renderTriggerRow)}
                </div>
              ))}
        </div>
      )}

      <TriggersBulk
        selectedTriggers={triggers.filter((t) => selSet.has(t.id))}
        selectedCount={selectedIds.length}
        pending={pending}
        setPending={setPending}
        onTriggersChanged={onTriggersChanged}
      />

    </div>
  );
}
