import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchPipelines } from "../api";
import { pickDefaultBranch } from "../lib/branchSelect";
import { useBranchList } from "./useBranchList";
import type { PipelineListEntry } from "../types";

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
  const [selectedPipelineId, setSelectedPipelineId] = useState("");
  const [sourceBranch, setSourceBranch] = useState("");
  const {
    branches,
    loading: branchesLoading,
    lastFetchAt,
    fetchError,
    fetching,
    load,
    refetch: refetchRemotes,
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
    selectedPipeline,
    selectedPipelineId,
    setSelectedPipelineId,
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
  };
}
