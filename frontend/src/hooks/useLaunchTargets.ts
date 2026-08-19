import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchPipelines, listBranches } from "../api";
import { pickDefaultBranch } from "../lib/branchSelect";
import type { BranchRef, PipelineListEntry } from "../types";

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
 */
export function useLaunchTargets(open: boolean) {
  const [pipelines, setPipelines] = useState<PipelineListEntry[]>([]);
  const [selectedPipelineId, setSelectedPipelineId] = useState("");
  const [branches, setBranches] = useState<BranchRef[]>([]);
  const [sourceBranch, setSourceBranch] = useState("");
  const [branchesLoading, setBranchesLoading] = useState(false);

  const clearBranches = useCallback(() => {
    setBranches([]);
    setSourceBranch("");
  }, []);

  const loadBranches = useCallback(
    async (repoPath: string) => {
      setBranchesLoading(true);
      try {
        const branchList = await listBranches(repoPath);
        setBranches(branchList);
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
        if (
          branchList.length > 0 &&
          !branchList.some((b) => b.name === sourceBranch)
        ) {
          const def = pickDefaultBranch(branchList);
          if (def) setSourceBranch(def);
        }
      } catch {
        setBranches([]);
      } finally {
        setBranchesLoading(false);
      }
    },
    [sourceBranch],
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

  const repoPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "repo"),
    [pipelines],
  );
  const libraryPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "library"),
    [pipelines],
  );
  const userPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "user"),
    [pipelines],
  );

  const selectedPipeline = useMemo(
    () => pipelines.find((p) => p.id === selectedPipelineId),
    [pipelines, selectedPipelineId],
  );

  return {
    pipelines,
    repoPipelines,
    libraryPipelines,
    userPipelines,
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
  };
}
