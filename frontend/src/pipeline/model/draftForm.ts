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

/**
 * The draft's pipeline as the editor holds it, its keys in the order of
 * `shown` (what the tab held before): the replicated document keeps no key
 * order, and a save writes the keys of `view: { x, y }` or of an edge end in
 * the order it reads them.
 */
export function fromDraftPipeline(json: unknown, shown?: EditorPipelineDef): EditorPipelineDef {
  const pipeline = orderedLike(json as Json, shown as unknown as Json) as unknown as EditorPipelineDef;
  return {
    ...pipeline,
    nodes: (pipeline.nodes ?? []).map(foldHarnessOntoNode),
    edges: pipeline.edges ?? [],
  };
}

function isObject(value: Json | undefined): value is { [key: string]: Json } {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/**
 * `value` with the keys of each object in the order of the same object in
 * `reference` (an array element is the one with the same `id`, else at the
 * same place), the keys `reference` lacks after them, sorted.
 */
export function orderedLike(value: Json, reference: Json | undefined): Json {
  if (Array.isArray(value)) {
    const previous = Array.isArray(reference) ? reference : [];
    const byId = new Map(
      previous.filter(isObject).filter((e) => typeof e.id === "string").map((e) => [e.id as string, e]),
    );
    return value.map((element, index) => {
      const id = isObject(element) && typeof element.id === "string" ? element.id : null;
      return orderedLike(element, id != null ? byId.get(id) : previous[index]);
    });
  }
  if (!isObject(value)) return value;
  const known = isObject(reference) ? Object.keys(reference).filter((key) => key in value) : [];
  const others = Object.keys(value).filter((key) => !known.includes(key)).sort();
  const out: { [key: string]: Json } = {};
  for (const key of [...known, ...others]) {
    out[key] = orderedLike(value[key], isObject(reference) ? reference[key] : undefined);
  }
  return out;
}
