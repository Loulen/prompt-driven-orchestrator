/**
 * What the preview of the Pipelines list block of Settings › Appearance shows:
 * the Pipeline the user sees edited last, and one with a draft when it has none
 * (else the next most recent), so that the draft marker appears once; a sample
 * Pipeline when the user sees none.
 */
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import { recentPair } from "./recentPair";

export interface PipelinesListPreview {
  /** The rows, in their order. */
  pipelines: PipelineListEntry[];
  /** A sample Pipeline: the user sees no Pipeline. */
  sample: boolean;
  caption: string;
}

/** The Pipeline the preview shows when the user sees none: no extension mark goes on it. */
export function samplePipeline(now: number): PipelineListEntry {
  return {
    id: "sample-pipeline",
    name: "my-pipeline",
    scope: "instance",
    path: "",
    node_count: 3,
    modified: new Date(now - 26 * 3_600_000).toISOString(),
    variables: {},
    prompt_required: false,
    validation: { kind: "all" },
    draft: { changes: 2, nodes: 3, participants: [] },
    version: { number: 3, title: "" },
  };
}

/** The rows of the preview, from the Pipelines the user sees. */
export function pipelinesListPreview(pipelines: PipelineListEntry[], now: number = Date.now()): PipelinesListPreview {
  if (pipelines.length === 0) {
    return { pipelines: [samplePipeline(now)], sample: true, caption: "Sample pipeline · you see no pipeline yet" };
  }
  return {
    pipelines: recentPair(pipelines, (p) => p.modified, (p) => (p.draft?.changes ?? 0) > 0),
    sample: false,
    caption: "Your pipelines",
  };
}
