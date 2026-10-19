/**
 * A pipeline as its shared draft holds it (ADR-0088) and as the editor holds
 * it. The draft keeps the daemon's form (`GET /pipelines/{id}`): each node's
 * per-harness `harnesses` map, no flat `model`/`effort`. The editor folds them
 * out on the way in (`foldPipelineDetail`) and back on the way out.
 */

import { foldHarnessOntoNode, foldNodeIntoHarnesses } from "../../harness/model/nodeHarness";
import type { EditorPipelineDef } from "./pipeline";
import type { Json } from "./draftDelta";

/** The editor's pipeline as the draft holds it: plain JSON, no `undefined`. */
export function toDraftPipeline(pipeline: EditorPipelineDef): { [key: string]: Json } {
  const nodes = pipeline.nodes.map((node) => {
    const { model: _model, effort: _effort, ...rest } = node;
    void _model;
    void _effort;
    return { ...rest, harnesses: foldNodeIntoHarnesses(node) ?? {} };
  });
  return JSON.parse(JSON.stringify({ ...pipeline, nodes })) as { [key: string]: Json };
}

/** The draft's pipeline as the editor holds it. */
export function fromDraftPipeline(json: unknown): EditorPipelineDef {
  const pipeline = json as EditorPipelineDef;
  return {
    ...pipeline,
    nodes: (pipeline.nodes ?? []).map(foldHarnessOntoNode),
    edges: pipeline.edges ?? [],
  };
}
