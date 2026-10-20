import { useEffect, useMemo, useRef, useState } from "react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import {
  ancestorIds, buildRunTree, filterRunTree, flattenAll, isInArchivedSection, parentIds, rootOf,
} from "../../model/runTree";
import { EMPTY_RUN_FILTER, isFilterActive, runMatchesFilter } from "../../model/runFilter";
import { loadChildRunsExpanded } from "../../../lib/uiPrefs";
import {
  isAttributeFilterActive,
  matchesAttributes,
  type Attributes,
} from "../../../shared/extensions/attributes";
import type { Project } from "../../../workspace/model/project";
import { recordedProject } from "../../../workspace/model/projectLookup";
import type { DeletedProject } from "../../../shared/generated/DeletedProject";

/**
 * The Runs list's tree view: the client-side filters, the run tree split into
 * its active / archived sections, the expand / collapse state and the reveal
 * of the selected run.
 */
export function useRunTreeView(
  runs: RunListEntry[],
  selectedRunId: string | null,
  projects: Project[] = [],
  /** The deleted Projets the Runs still name (CORE-96). */
  deletedProjects: DeletedProject[] = [],
) {
  // #336 — client-side run filters (project / pipeline / trigger / date), AND
  // semantics, session-only state. #783: applied to the TREE (a non-matching
  // parent is kept as a path to a matching descendant) and BEFORE the
  // active/archived split, so grouping, the Archived section and its count all
  // see the same filtered view.
  const [runFilter, setRunFilter] = useState(EMPTY_RUN_FILTER);
  // ADR-0081 — the attribute filter an extension's list filter sets: kept on
  // the same tree, with the same AND semantics.
  const [attributeFilter, setAttributeFilter] = useState<Attributes>({});
  // ADR-0081 (#31) — the `projects/filter` of the list: it keeps the Runs whose
  // root belongs to a Projet carrying every attribute asked for (the list
  // groups by Projet on the root's repository, so does this).
  const [projectFilter, setProjectFilter] = useState<Attributes>({});
  const filterActive =
    isFilterActive(runFilter) ||
    isAttributeFilterActive(attributeFilter) ||
    isAttributeFilterActive(projectFilter);

  // #783 — the run tree, built from ALL runs so a child follows its parent into
  // whatever group the ROOT lands in, whatever the child's own repo. A subtree
  // never spans two sections (`buildRunTree` makes a run whose parent sits in
  // the other section a root, linked to that parent), so the split on the
  // root's status leaves the Archived section with archived runs only, nested
  // like the active ones.
  const fullTree = useMemo(() => buildRunTree(runs), [runs]);
  const runById = useMemo(() => new Map(runs.map((r) => [r.run_id, r])), [runs]);
  const tree = useMemo(() => {
    if (!isAttributeFilterActive(projectFilter)) return fullTree;
    const byId = new Map(projects.map((p) => [p.id, p]));
    return fullTree.filter((n) => {
      const project = n.run.project_id ? byId.get(n.run.project_id) : undefined;
      return project !== undefined && matchesAttributes(project.attributes, projectFilter);
    });
  }, [fullTree, projects, projectFilter]);
  // ADR-0087 — the Projet recorded on a Run: the list's groups and the
  // filter's Project axis read it alike.
  const projectOfRun = useMemo(
    () => recordedProject(projects, deletedProjects),
    [projects, deletedProjects],
  );
  const { activeTree, archivedTree } = useMemo(() => {
    // The Date axis places its presets from now: re-read at each change of
    // the Runs, which a live list sees often.
    const now = new Date();
    const match = (r: RunListEntry) =>
      runMatchesFilter(r, runFilter, projectOfRun, now) &&
      matchesAttributes(r.attributes, attributeFilter);
    return {
      activeTree: filterRunTree(tree.filter((n) => !isInArchivedSection(n.run)), match),
      archivedTree: filterRunTree(tree.filter((n) => isInArchivedSection(n.run)), match),
    };
  }, [tree, runFilter, attributeFilter, projectOfRun]);
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
  const allParentIds = useMemo(() => parentIds(fullTree), [fullTree]);

  // #725/#783 — reveal the selected run: when the selection CHANGES to a run
  // whose ancestors are collapsed (Orchestration tab, URL, back navigation),
  // open them and scroll the row into view once it exists. A root reached by
  // a parent link (possibly in the other section) is scrolled to as well. Adjusting state
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
      setScrollRequest(selectedRunId);
    }
  }
  useEffect(() => {
    if (scrollRequest === null || selectedRunId !== scrollRequest) return;
    runsScrollRef.current
      ?.querySelector(`[data-run-row="${scrollRequest}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [scrollRequest, selectedRunId]);

  // #136 — archived runs live in their own collapsible section below the
  // active list; the active list keeps the #258 per-repo grouping. The section
  // renders the tree like the active list; its count is every archived run it
  // holds, nested or not.
  const archivedCount = useMemo(() => flattenAll(archivedTree).length, [archivedTree]);

  // Identity of the selected run *iff* its tree currently sits in the archived
  // section (its root is archived — so is the run itself, a subtree never
  // spanning two sections), else null — the signal that must reveal the section.
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

  return {
    projectOfRun,
    runFilter,
    setRunFilter,
    attributeFilter,
    setAttributeFilter,
    projectFilter,
    setProjectFilter,
    filterActive,
    activeTree,
    archivedTree,
    filteredRuns,
    runById,
    isExpanded,
    setExpanded,
    allParentIds,
    runsScrollRef,
    archivedCount,
    archivedOpen,
    setArchivedOpen,
    allExpanded,
    setAllExpanded,
  };
}
