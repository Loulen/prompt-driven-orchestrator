import type {
  PipelineDef,
  NodeDef,
  PortDef,
  EdgeDef,
  LoopRegion,
  NoteDef,
} from "../types";

/**
 * Single owner (#355) of the emit-surface taxonomy: for every scope
 * `pipelineToYamlObject` emits, which keys are SEMANTIC (part of the pipeline's
 * meaning — compared behind the library synced/diverged star) vs LAYOUT (canvas
 * presentation — persisted in the file so a shared workflow keeps its positions,
 * pinned routes, arrow sides and notes, but excluded from the semantic diff).
 *
 * The invariant, enforced by `layoutFields.test.ts`, is per scope:
 *
 *     Object.keys(emitted[scope])  ==  SEMANTIC_FIELDS[scope] ∪ LAYOUT_FIELDS[scope]
 *
 * Add a field to the serializer (`editStore.ts` pipelineToYamlObject) → classify
 * it here, or the exhaustiveness guard turns the build RED.
 *
 * `LAYOUT_FIELDS` has a runtime consumer (`stripLayout`, used by
 * `comparablePipelineObject`). `SEMANTIC_FIELDS` has NO runtime consumer — the
 * emitter already includes every field; this half exists only to make the
 * guard's set-equality exact. Do NOT build a "keep only semantic" path from it.
 *
 * Each array is written `satisfies (keyof X)[]` so renaming or removing a listed
 * type field fails `tsc` (a compile-time tripwire that backs the runtime guard).
 */
export type SerializerScope =
  | "pipeline"
  | "node"
  | "inputPort"
  | "outputPort"
  | "edge"
  | "loopRegion"
  | "note";

export const SEMANTIC_FIELDS: Record<SerializerScope, readonly string[]> = {
  pipeline: ["name", "version", "prompt_required", "variables", "nodes", "edges", "loops"] satisfies (keyof PipelineDef)[],
  node: ["id", "name", "type", "interactive", "model", "max_iter", "inputs", "outputs"] satisfies (keyof NodeDef)[],
  // `side` is SEMANTIC today (emitted, not stripped) — deliberate, see #355 D5:
  // the node-library star already treats port side as identity, so the pipeline
  // diff must agree or the two stars contradict each other on the same edit.
  inputPort: ["name", "repeated", "side", "port_type", "frontmatter"] satisfies (keyof PortDef)[],
  outputPort: ["name", "repeated", "side", "port_type", "frontmatter", "when"] satisfies (keyof PortDef)[],
  edge: ["source", "target", "when", "else"] satisfies (keyof EdgeDef)[],
  loopRegion: ["id", "kind", "members", "max_iter", "over"] satisfies (keyof LoopRegion)[],
  note: [], // the whole `notes` block is layout — no semantic note field
};

export const LAYOUT_FIELDS: Record<SerializerScope, readonly string[]> = {
  pipeline: ["notes"] satisfies (keyof PipelineDef)[],
  node: ["view"] satisfies (keyof NodeDef)[],
  inputPort: [],
  outputPort: [],
  edge: ["mode", "waypoints", "target_side"] satisfies (keyof EdgeDef)[],
  loopRegion: [],
  // GUARD-ONLY: the strip drops the whole `notes` block via LAYOUT_FIELDS.pipeline
  // and never descends into a note (ADR-0018 R1 — notes are whole-block layout).
  // These are listed only so the exhaustiveness guard covers the note scope too.
  note: ["id", "content", "view"] satisfies (keyof NoteDef)[],
};

/**
 * Reduce a serialized pipeline to its semantic core by removing every LAYOUT
 * field. MUTATES and returns `obj`.
 *
 * Intended to be fed the fresh, throwaway output of `pipelineToYamlObject` (a new
 * object graph per call — `nodes`/`edges`/`notes` are all new `.map(...)` arrays),
 * so in-place deletion is safe and avoids a deep clone on the star-divergence
 * path. Do NOT pass a shared/persisted object. Only top-level KEYS are removed —
 * `delete node.view` unlinks the copied reference; it never mutates the shared
 * `view` object.
 *
 * Consumes LAYOUT_FIELDS at the scopes that actually appear in the compared
 * structure (pipeline/node/edge). Ports and loop regions have no layout fields;
 * notes are removed wholesale at pipeline scope (`notes`), so the strip never
 * descends into node/loop/note internals.
 */
export function stripLayout(obj: Record<string, unknown>): Record<string, unknown> {
  for (const k of LAYOUT_FIELDS.pipeline) delete obj[k]; // notes (whole block)

  const nodes = obj.nodes;
  if (Array.isArray(nodes)) {
    for (const node of nodes as Record<string, unknown>[]) {
      for (const k of LAYOUT_FIELDS.node) delete node[k]; // view
    }
  }
  const edges = obj.edges;
  if (Array.isArray(edges)) {
    for (const edge of edges as Record<string, unknown>[]) {
      for (const k of LAYOUT_FIELDS.edge) delete edge[k]; // mode, waypoints, target_side
    }
  }
  return obj;
}
