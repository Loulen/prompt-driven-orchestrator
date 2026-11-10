import { useMemo, useState } from "react";
import { CalendarClock, Funnel, Plus } from "lucide-react";
import type { Trigger } from "../../shared/generated/Trigger";
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";
import type { Project } from "../../workspace/model/project";
import { deleteTrigger, updateTrigger } from "../api/triggers";
import { groupByProject } from "../../lib/groupByRepo";
import { recordedProject } from "../../workspace/model/projectLookup";
import { useSelectionStore } from "../../stores/selectionStore";
import { handleSelectionKeydown } from "../../lib/selectionKeys";
import { projectsEntryOf, type ProjectsEntry } from "../../workspace/model/projectsSettings";
import GroupHeader from "../../shared/ui/leftList/GroupHeader";
import ListEmpty from "../../shared/ui/leftList/ListEmpty";
import TriggerRow from "./triggersList/TriggerRow";
import ConfirmDeleteModal from "../../shared/ui/ConfirmDeleteModal";
import { toast } from "../../shared/ui/toast";
import TriggersBulk, { type TriggerBulkKind } from "./triggersList/TriggersBulk";
import TriggersListHeader from "./triggersList/TriggersListHeader";
import { useTriggerListRefusals } from "../store/useTriggerRefusals";
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
  // The Projet groups folded by their header, for this session.
  const [collapsedGroups, setCollapsedGroups] = useState<ReadonlySet<string>>(() => new Set());
  const toggleGroup = (key: string) =>
    setCollapsedGroups((folded) => {
      const next = new Set(folded);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

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

  // CORE-205: the gestures the caller may not make, asked once for the list;
  // a refused one stays, greyed, with the refusal's sentence.
  const projectIds = useMemo(() => projects.map((p) => p.id), [projects]);
  const refusals = useTriggerListRefusals(allTriggers, projectIds);
  const selectedTriggers = triggers.filter((t) => selSet.has(t.id));
  const bulkDeleteRefused =
    selectedTriggers.length > 0 && selectedTriggers.every((t) => refusals.of(t.id).delete !== undefined);

  // A deletion loses the schedule and the fire history: confirmed first, as the
  // bulk delete is (CORE-232).
  const [deleting, setDeleting] = useState<Trigger | null>(null);

  async function handleDelete(t: Trigger) {
    setDeleting(null);
    try {
      await deleteTrigger(t.id);
      onTriggersChanged();
    } catch (e) {
      toast(`Delete ${t.name}: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }

  async function handleToggle(t: Trigger) {
    try {
      await updateTrigger(t.id, { enabled: !t.enabled });
      onTriggersChanged();
    } catch (e) {
      toast(`${t.enabled ? "Disable" : "Enable"} ${t.name}: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }

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
      : triggerGroups.flatMap((g) => (collapsedGroups.has(g.key) ? [] : g.items.map((t) => t.id)));

  const renderTriggerRow = (t: Trigger) => (
    <TriggerRow
      key={t.id}
      trigger={t}
      isSelected={t.id === selectedTriggerId}
      rowSelected={selSet.has(t.id)}
      selecting={selectedIds.length > 0}
      paused={paused}
      onSelect={() => onSelectTrigger(t.id)}
      onSelectControl={(shiftKey) => {
        if (shiftKey) selectRange("triggers", t.id, orderedIds);
        else toggle("triggers", t.id);
      }}
      onRunNow={onRunNow && (() => onRunNow(t))}
      onEdit={onEditTrigger && (() => onEditTrigger(t))}
      onDelete={() => setDeleting(t)}
      onToggle={() => void handleToggle(t)}
      refused={refusals.of(t.id)}
    />
  );

  return (
    <div className="lp-pane" data-testid="triggers-list-panel">
      <TriggersListHeader
        paused={paused}
        onTogglePause={onTogglePause}
        onNewTrigger={onNewTrigger}
        pauseRefused={refusals.pause}
        createRefused={refusals.create}
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
        <ListEmpty
          icon={Funnel}
          title="No match"
          actionLabel="Clear filters"
          onAction={() => {
            setAttributeFilter({});
            setProjectFilter({});
          }}
          testId="trigger-filter-empty"
        />
      ) : triggers.length === 0 ? (
        <ListEmpty
          icon={CalendarClock}
          title="No triggers"
          actionLabel="New trigger"
          actionIcon={Plus}
          onAction={onNewTrigger}
          actionRefused={refusals.create}
          testId="triggers-empty"
        />
      ) : (
        <div
          className="lp-scroll"
          tabIndex={-1}
          onKeyDown={(e) =>
            handleSelectionKeydown(e, {
              tab: "triggers",
              visibleIds: orderedIds,
              hasSelection: selectedIds.length > 0,
              selectVisible,
              clear: clearSel,
              onBulkDelete: () => {
                if (selectedIds.length > 0 && !bulkDeleteRefused) setPending("delete");
              },
            })
          }
        >
          {triggerGroups === null ? (
            <div className="lp-rows">{triggers.map(renderTriggerRow)}</div>
          ) : (
            triggerGroups.map((group) => {
              const groupIds = group.items.map((t) => t.id);
              const collapsed = collapsedGroups.has(group.key);
              return (
                <section
                  key={group.key}
                  className="lp-section"
                  data-testid="trigger-repo-group"
                  data-project={group.kind === "project" ? "true" : "false"}
                  data-collapsed={collapsed ? "true" : "false"}
                >
                  <GroupHeader
                    label={group.label}
                    title={group.title}
                    count={group.items.length}
                    deleted={group.deleted}
                    collapsed={collapsed}
                    onToggle={() => toggleGroup(group.key)}
                    // The group-header "select all in this Projet" control (#577).
                    allSelected={groupIds.length > 0 && groupIds.every((id) => selSet.has(id))}
                    onSelectAll={() => selectGroup("triggers", groupIds)}
                    // #552 — the pencil: Settings › Projects on this group (CORE-96).
                    onEdit={
                      !group.deleted && onOpenProjectSettings
                        ? () => onOpenProjectSettings(projectsEntryOf(group))
                        : undefined
                    }
                    editLabel={group.kind === "project" ? "Edit project" : "Name project"}
                    testPrefix="trigger-group"
                    labelTestId="trigger-repo-label"
                  />
                  {!collapsed && <div className="lp-rows">{group.items.map(renderTriggerRow)}</div>}
                </section>
              );
            })
          )}
        </div>
      )}

      <TriggersBulk
        selectedTriggers={selectedTriggers}
        selectedCount={selectedIds.length}
        pending={pending}
        setPending={setPending}
        onTriggersChanged={onTriggersChanged}
        refusalsOf={refusals.of}
      />

      <ConfirmDeleteModal
        key={deleting?.id ?? ""}
        open={deleting !== null}
        kind="trigger"
        name={deleting?.name ?? ""}
        detail="Its schedule and fire history go"
        onClose={() => setDeleting(null)}
        onConfirm={() => deleting && void handleDelete(deleting)}
      />
    </div>
  );
}
