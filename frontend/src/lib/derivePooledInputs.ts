import type { PipelineDef } from "../types";
import { carriedPorts, emergentInputs } from "./edgePorts";

/** One source node contributing to a pooled input. */
export interface PooledInputSource {
  /** The contributing source node's id. */
  nodeId: string;
  /** Display label for the source node (its name, falling back to its id). */
  label: string;
  /** Index of the contributing edge in `pipeline.edges` — the handle for
   * per-source deletion (#339). Re-derived every render, so it never goes
   * stale across mutations. */
  edgeIndex: number;
  /** The source output port this input carries (ADR-0073 / #843). */
  port: string;
  /**
   * True when the contributing edge carries OTHER outputs too, each of which is
   * its own input on this node. Deleting such a source must untick this port,
   * not remove the edge — removing it would silently take the sibling inputs
   * with it.
   */
  sharedEdge: boolean;
}

/**
 * A derived (emergent) input on a node. Inputs are NOT declared (#149,
 * CONTEXT.md § Node): they are derived from incoming edges, named after the
 * source document. Several same-named incoming edges POOL into one logical list
 * input that lists every contributing source node — e.g.
 * `review ← security-reviewer, perf-reviewer`.
 *
 * One incoming edge may produce SEVERAL inputs: an edge carrying two outputs
 * (ADR-0073 / #843) drops one input per carried port.
 */
export interface PooledInput {
  /** The emergent input name (inherited from the source document). */
  name: string;
  /** `repeated` is read off the edge (accumulate `iter-*`); true if any pooled edge sets it. */
  repeated: boolean;
  /** Every source node feeding this pooled input, in edge declaration order. */
  sources: PooledInputSource[];
}

/**
 * Derives the pooled, emergent inputs of a node from a pipeline's edges. The
 * inspector renders this read-only list — the node itself declares no inputs.
 */
export function derivePooledInputs(pipeline: PipelineDef, nodeId: string): PooledInput[] {
  const labelOf = (id: string): string => {
    const n = pipeline.nodes.find((nd) => nd.id === id);
    return n?.name ?? id;
  };

  const byName = new Map<string, PooledInput>();
  for (const [edgeIndex, edge] of pipeline.edges.entries()) {
    if (edge.target.node !== nodeId) continue;
    const sharedEdge = carriedPorts(edge.source).length > 1;
    // One input per CARRIED port (ADR-0073): a single-port edge yields its
    // target port, a multi-port one yields an input named after each output.
    for (const { port, input: name } of emergentInputs(edge)) {
      let input = byName.get(name);
      if (!input) {
        input = { name, repeated: false, sources: [] };
        byName.set(name, input);
      }
      input.sources.push({
        nodeId: edge.source.node,
        label: labelOf(edge.source.node),
        edgeIndex,
        port,
        sharedEdge,
      });
      if (edge.repeated) input.repeated = true;
    }
  }

  return [...byName.values()];
}
