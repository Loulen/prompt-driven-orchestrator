import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchPipelines } from "../../pipeline/api/pipelines";
import { fetchProjects } from "../../workspace/api/projects";
import type { Project } from "../../workspace/model/project";
import { pickDefaultBranch } from "../../workspace/model/branchSelect";
import { useBranchList } from "../../workspace/store/useBranchList";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";

/**
 * What a Run/Trigger can be launched AGAINST (#359): the target repo's branches and the
 * pipelines the instance serves. Both are lists the daemon owns, so both live here,
 * behind the same hook the New Run modal drives.
 *
 * Branch loading is *called*, not reactive: `useRepoValidation` owns the debounce and the
 * verdict, and hands the validated path over once — `loadBranches` on a valid repo,
 * `clearBranches` on an invalid one. Keeping the two as callbacks (rather than an effect
 * keyed on the verdict) preserves the single async chain the modal has always had: the
 * repo is still "validating" while its branches load.
 *
 * #802/ADR-0070: loading a repo's branches also FETCHES its remotes. The écart amont the
 * form shows is only worth the date it carries, and "the last time the operator happened
 * to fetch" is not a date the form can defend. `useBranchList` owns the fetch and its
 * races; what stays here is the SELECTION — which repo's branch is held, and when it is
 * re-seeded.
 */
export function useLaunchTargets(open: boolean) {
  const [pipelines, setPipelines] = useState<PipelineListEntry[]>([]);
  // The Projets, to know which one owns the chosen repository and so which
  // Pipelines head the menu (CONTEXT.md « Pipeline validé »). `null` until the
  // first answer: a selection must not be judged « not offered » against a list
  // that has not arrived yet.
  const [projects, setProjects] = useState<Project[] | null>(null);
  const [selectedPipelineId, setSelectedPipelineId] = useState("");
  // Whether the held pipeline is the user's choice (picked in the menu, or the
  // edited Trigger's) rather than the form's default. Only a choice is kept; a
  // default follows the menu.
  const [pipelineChosen, setPipelineChosen] = useState(false);
  const [sourceBranch, setSourceBranch] = useState("");
  const {
    branches,
    loading: branchesLoading,
    lastFetchAt,
    fetchError,
    fetching,
    load,
    refetch: refetchRemotes,
    reread: rereadBranches,
    fastForward,
    clear,
  } = useBranchList();

  const clearBranches = useCallback(() => {
    clear();
    setSourceBranch("");
  }, [clear]);

  const loadBranches = useCallback(
    async (repoPath: string) => {
      const list = await load(repoPath);
      if (!list) return;
      // #454: re-select whenever the held branch is not one THIS repo has.
      // The old `!sourceBranch` guard only ever seeded an empty field, so
      // switching repos kept a branch the new one lacks — and a `<select>`
      // whose value matches no option renders its FIRST option, so the field
      // DISPLAYED `master` while the state still held `main`. The launch then
      // failed with `branch 'main' does not exist`, blaming the daemon for a
      // value the UI never showed. Testing membership instead subsumes the
      // empty case and still preserves a deliberate choice the new repo honours.
      // #571: membership is on `name` (the verbatim value posted); the default
      // is locality-aware (see `pickDefaultBranch`) so a remote never wins over
      // an available local.
      //
      // #802: this runs on a REPO CHANGE only — never after a refetch. A refetch
      // answers "how fresh is this?", and re-seeding there would yank the field
      // out from under someone who had just picked `origin/main` in the sync
      // popover (an upstream ref the list deliberately does not carry, #571).
      if (list.length > 0 && !list.some((b) => b.name === sourceBranch)) {
        const def = pickDefaultBranch(list);
        if (def) setSourceBranch(def);
      }
    },
    [load, sourceBranch],
  );

  const loadPipelines = useCallback(() => {
    if (!open) return;
    fetchPipelines()
      .then((list) => setPipelines(list))
      .catch(() => {});
    // Fresh on every open: a Projet created meanwhile (the First run tour makes
    // one) must head the menu the next time the form opens.
    fetchProjects()
      .then((list) => setProjects(list))
      .catch(() => setProjects([]));
  }, [open]);

  useEffect(() => {
    loadPipelines();
  }, [loadPipelines]);

  const selectedPipeline = useMemo(
    () => pipelines.find((p) => p.id === selectedPipelineId),
    [pipelines, selectedPipelineId],
  );

  return {
    pipelines,
    projects,
    selectedPipeline,
    selectedPipelineId,
    setSelectedPipelineId,
    pipelineChosen,
    setPipelineChosen,
    loadPipelines,
    branches,
    branchesLoading,
    sourceBranch,
    setSourceBranch,
    loadBranches,
    clearBranches,
    lastFetchAt,
    fetchError,
    fetching,
    refetchRemotes,
    rereadBranches,
    fastForward,
  };
}
