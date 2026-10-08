import { useMemo } from "react";
import {
  defaultPipeline,
  isOffered,
  pipelineSections,
  projectOfRepo,
} from "../../../pipeline/model/pipelineValidation";
import type { NewRunState } from "./useNewRunState";

/**
 * What the Pipeline menu of New Run / Trigger offers (CONTEXT.md « Pipeline
 * validé »): the Pipelines of the primary repository's Projet first, then the
 * global ones; test Pipelines wait behind « Show test pipelines ». The Projet is
 * matched on the path verbatim. Filtering is the form's only: the daemon launches
 * any Pipeline it is given.
 *
 * Also holds the selection to it, during render (the form's own pattern): a
 * Pipeline validated for other Projets only is dropped, and until the user picks
 * one the selection follows the default — the Projet's first Pipeline, else the
 * first global one, never a test one.
 */
export function usePipelineOffer(open: boolean, state: NewRunState) {
  const {
    pipelines, projects, repoValid, targetRepo, selectedPipeline, selectedPipelineId,
    setSelectedPipelineId, pipelineChosen, setPipelineChosen, setOverrides,
  } = state;

  const repoProject = useMemo(
    () => (repoValid && projects ? projectOfRepo(projects, targetRepo) : null),
    [projects, repoValid, targetRepo],
  );
  const sections = useMemo(
    () => pipelineSections(pipelines, repoProject?.id ?? null),
    [pipelines, repoProject],
  );

  // Judged only once the Projets are known: a choice held from another
  // repository is dropped, and the default below takes over.
  if (
    open &&
    repoValid &&
    projects != null &&
    selectedPipeline &&
    !isOffered(selectedPipeline, repoProject?.id ?? null)
  ) {
    setSelectedPipelineId("");
    setPipelineChosen(false);
  }

  const fallback = defaultPipeline(sections);
  const shouldAutoSelect =
    open &&
    repoValid &&
    projects != null &&
    fallback != null &&
    (!selectedPipelineId || (!pipelineChosen && selectedPipelineId !== fallback.id));
  if (shouldAutoSelect) {
    // A default replaced by another one takes nothing along: its overrides were
    // typed for the pipeline being left.
    if (selectedPipelineId) setOverrides({});
    setSelectedPipelineId(fallback.id);
  }

  return { repoProject, sections };
}
