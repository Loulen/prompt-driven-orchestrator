import { useCallback, useMemo, useState } from "react";
import { isLiveRun } from "../../model/runStatus";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { Project } from "../../../workspace/model/project";
import { useSelectionStore } from "../../../stores/selectionStore";
import { useRecentReposStore } from "../../../stores/recentReposStore";
import { type BulkItem, type BulkOutcome } from "../../../lib/bulk";
import { groupByProject, type ProjectRef } from "../../../lib/groupByRepo";
import { cascadeImpact, flattenVisible, type RunTreeNode } from "../../model/runTree";
import { projectLookup } from "../../../workspace/model/projectLookup";
import { RETRYABLE } from "../../components/runsList/runsListTables";
import { useRunRowActions } from "./useRunRowActions";
import { useRunTreeView } from "./useRunTreeView";
import { useGestureRightsStore, useListedRunGestures } from "../gestureRightsStore";
import { mayOnRun, type RunGesture } from "../../model/runGestures";

export interface ProjectEditorState {
  project: Project | null;
  name: string;
  memberPaths: string[];
}

export interface ProjectEditorTarget {
  kind: "project" | "path";
  key: string;
  repoPath: string;
  label: string;
}

/**
 * All the state of the Runs tab. It lives in `UnifiedLeftPanel` (not in the
 * pane) so the list's filters, expansion and pending confirms survive a tab
 * switch, as they always did.
 */
export function useRunsPane({
  runs,
  selectedRunId,
  onSelectRun,
  projects,
}: {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  projects: Project[];
}) {
  const actions = useRunRowActions(onSelectRun);
  // #552 — the group-header pencil. Holds the Projet being edited (or `null` for
  // a fresh one, when the header is a derived path group) plus the pre-fill.
  const [projectEditor, setProjectEditor] = useState<ProjectEditorState | null>(null);

  const recentRepos = useRecentReposStore((s) => s.recentRepos);

  // #577 — multi-select. The per-tab sets live in the shared store (so a tab's
  // count survives a switch as its badge); the pending bulk action is local.
  const runSelIds = useSelectionStore((s) => s.runs);
  const toggleSel = useSelectionStore((s) => s.toggle);
  const selectRange = useSelectionStore((s) => s.selectRange);
  const selectVisible = useSelectionStore((s) => s.selectVisible);
  const selectGroup = useSelectionStore((s) => s.selectGroup);
  const deselect = useSelectionStore((s) => s.deselect);
  const clearSel = useSelectionStore((s) => s.clear);
  const runSel = useMemo(() => new Set(runSelIds), [runSelIds]);
  const [runBulkKind, setRunBulkKind] = useState<"cleanup" | "retry" | "pause" | null>(null);

  const view = useRunTreeView(runs, selectedRunId, projects);

  // CORE-42: the gestures of each row the caller may make, asked once per
  // Run; a refused one is hidden from the row, its menu and the bulk bar.
  const runIds = useMemo(() => runs.map((r) => r.run_id), [runs]);
  useListedRunGestures(runIds);
  const gestureRights = useGestureRightsStore((s) => s.rights);
  const mayOnRow = useCallback(
    (runId: string, gesture: RunGesture) => mayOnRun(gestureRights, runId, gesture),
    [gestureRights],
  );
  const { activeTree, archivedTree, filteredRuns, isExpanded, archivedOpen } = view;

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
  // CORE-42: and only the Runs where the caller may make it; an action no
  // selected Run allows is hidden (`bulkShown`).
  const allowing = (gesture: RunGesture) => selectedRuns.filter((r) => mayOnRow(r.run_id, gesture));
  const cleanupItems = allowing("cleanup_run").filter((r) => r.status !== "archived").map(asRunItem);
  const retryItems = allowing("retry_all").filter((r) => RETRYABLE.includes(r.status)).map(asRunItem);
  const pauseItems = allowing("pause_run")
    .filter((r) => r.status === "running" || r.status === "awaiting_user")
    .map(asRunItem);
  const bulkShown = {
    cleanup: allowing("cleanup_run").length > 0,
    retry: allowing("retry_all").length > 0,
    pause: allowing("pause_run").length > 0,
  };
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
  const openProjectEditor = (group: ProjectEditorTarget) => {
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

  return {
    ...actions,
    ...view,
    projectEditor,
    setProjectEditor,
    openProjectEditor,
    runSelIds,
    runSel,
    toggleSel,
    selectRange,
    selectVisible,
    selectGroup,
    clearSel,
    runBulkKind,
    setRunBulkKind,
    projectOf,
    availableRepos,
    runGroups,
    visibleRunIds,
    cleanupItems,
    retryItems,
    pauseItems,
    bulkShown,
    mayOnRow,
    runningWillStop,
    cleanupCascade,
    handleRunsSettled,
  };
}

export type RunsPaneState = ReturnType<typeof useRunsPane>;
