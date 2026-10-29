import type { OpenPipeline } from "../store/editState";
import type { RunListEntry } from "../../shared/generated/RunListEntry";

/** The name a tab shows: a Run's name, a pipeline's file, a version's file and number. */
export function tabTitle(tab: OpenPipeline, run: RunListEntry | undefined): string {
  if (tab.runId != null) return run?.name || tab.pipeline.name || tab.runId;
  // ADR-0088: a version shown read only is named after its pipeline.
  return tab.version ? `${tab.version.pipelineId}.yaml @ v${tab.version.number}` : `${tab.id}.yaml`;
}
