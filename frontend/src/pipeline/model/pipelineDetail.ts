/**
 * The editor's view of a loaded pipeline: the daemon's per-harness settings
 * folded onto each node (#550 / ADR-0046).
 */

import { foldHarnessOntoNode } from "../../harness/model/nodeHarness";
import type { PipelineDetail } from "./pipeline";

// #550/ADR-0046: the daemon returns each node's per-harness `harnesses` map; the
// editor's pickers edit a flat `model`/`effort` view of the RESOLVED harness. Fold
// on the way in so the existing UI + library sync keep working; `serializePipeline`
// folds back on save. Applied at every pipeline-load boundary.
export function foldPipelineDetail(detail: PipelineDetail): PipelineDetail {
  if (!detail?.pipeline?.nodes) return detail;
  return {
    ...detail,
    pipeline: {
      ...detail.pipeline,
      nodes: detail.pipeline.nodes.map(foldHarnessOntoNode),
    },
  };
}
