import { useMemo } from "react";
import {
  defaultPipeline,
  isOffered,
  pipelineSections,
} from "../../../pipeline/model/pipelineValidation";
import type { Project } from "../../../workspace/model/project";
import type { NewRunState } from "./useNewRunState";

/**
 * What the Pipeline menu of New Run / Trigger offers (CONTEXT.md « Pipeline
 * validé »): the Pipelines of the Projet the Run is launched in first (ADR-0087),
 * then the global ones; test Pipelines wait behind « Show test pipelines ».
 * Filtering is the form's only: the daemon launches any Pipeline it is given.
 *
 * Also holds the selection to it, during render (the form's own pattern): a
 * Pipeline validated for other Projets only is dropped, and until the user picks
 * one the selection follows the default — the Projet's first Pipeline, else the
 * first global one, never a test one.
 */
export function usePipelineOffer(open: boolean, state: NewRunState, launchProject: Project | null) {
  const {
    pipelines, projects, repoValid, selectedPipeline, selectedPipelineId,
    setSelectedPipelineId, pipelineChosen, setPipelineChosen, setOverrides,
  } = state;

  const repoProject = launchProject;
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
