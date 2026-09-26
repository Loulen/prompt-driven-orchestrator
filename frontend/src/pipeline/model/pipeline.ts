/**
 * Hand-written pipeline shapes: the `GET /pipelines/{id}` envelope (built with
 * `json!` on the daemon side) and editor-only notions.
 */

import type { NodeDef } from "../../shared/generated/NodeDef";
import type { PipelineDef } from "../../shared/generated/PipelineDef";

/**
 * A node as the editor holds it: the daemon's `NodeDef`, plus the flat
 * `model`/`effort` the pickers edit. They are folded out of `harnesses` on load
 * and merged back into it on save (`lib/harness.ts`); the daemon never sees
 * them. A `name` the user cleared is `null` here and falls back to the node's id
 * on save (`lib/serializePipeline.ts`): the daemon always receives a name.
 */
export type EditorNodeDef = Omit<NodeDef, "name"> & {
  name: string | null;
  model?: string | null;
  effort?: string | null;
};

/** A pipeline as the editor holds it: its nodes carry the folded model/effort. */
export type EditorPipelineDef = Omit<PipelineDef, "nodes"> & { nodes: EditorNodeDef[] };

/**
 * Runtime trigger status for a single conditional edge (ADR-0011, #147).
 * Shown ONLY in the edge detail panel — never rendered on the canvas. Derived
 * from the run state; absent until the edge's source node has been evaluated.
 */
export interface EdgeTriggerStatus {
  fired: boolean;
  /** The clause's evaluated value rendered for display, e.g. `verdict = FAIL`. */
  last_value: string | null;
  evaluated_at: string | null;
  iter: number | null;
}

export type EditScope = null | "run";

export type PipelineScope = "instance" | "repo" | "user" | "library";

export interface PipelineDetail {
  id: string;
  scope: PipelineScope;
  path: string;
  yaml: string;
  /** As the fetch wrappers hand it: nodes folded for the editor (`api.ts`). */
  pipeline: EditorPipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
}
