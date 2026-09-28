import { useEffect, useMemo, useRef, useState } from "react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import {
  ancestorIds, buildRunTree, filterRunTree, flattenAll, parentIds, rootOf,
} from "../../model/runTree";
import { EMPTY_RUN_FILTER, isFilterActive, runMatchesFilter } from "../../model/runFilter";
import { loadChildRunsExpanded } from "../../../lib/uiPrefs";
import {
  isAttributeFilterActive,
  matchesAttributes,
  type Attributes,
} from "../../../shared/extensions/attributes";

/**
 * The Runs list's tree view: the client-side filters, the run tree split into
 * its active / archived sections, the expand / collapse state and the reveal
 * of the selected run.
 */
export function useRunTreeView(runs: RunListEntry[], selectedRunId: string | null) {
  // #336 — client-side run filters (project / pipeline / trigger), AND
  // semantics, session-only state. #783: applied to the TREE (a non-matching
  // parent is kept as a path to a matching descendant) and BEFORE the
  // active/archived split, so grouping, the Archived section and its count all
  // see the same filtered view.
  const [runFilter, setRunFilter] = useState(EMPTY_RUN_FILTER);
  // ADR-0081 — the attribute filter an extension's list filter sets: kept on
  // the same tree, with the same AND semantics.
  const [attributeFilter, setAttributeFilter] = useState<Attributes>({});
  const filterActive = isFilterActive(runFilter) || isAttributeFilterActive(attributeFilter);

  // #783 — the run tree, built from ALL runs so a child follows its parent into
  // whatever section / group the ROOT lands in, whatever the child's own status
  // or repo. The archived split is on the root — and an archived run is always
  // a leaf (`buildRunTree` releases its children as roots, ADR-0064), so the
  // Archived section only ever holds archived runs, never a live child.
  const tree = useMemo(() => buildRunTree(runs), [runs]);
  const { activeTree, archivedTree } = useMemo(() => {
    const match = (r: RunListEntry) =>
      runMatchesFilter(r, runFilter) && matchesAttributes(r.attributes, attributeFilter);
    return {
      activeTree: filterRunTree(tree.filter((n) => n.run.status !== "archived"), match),
      archivedTree: filterRunTree(tree.filter((n) => n.run.status === "archived"), match),
    };
  }, [tree, runFilter, attributeFilter]);
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

  return {
    runFilter,
    setRunFilter,
    attributeFilter,
    setAttributeFilter,
    filterActive,
    activeTree,
    archivedTree,
    filteredRuns,
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
