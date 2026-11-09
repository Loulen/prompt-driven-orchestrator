import type { EdgeDef } from "../shared/generated/EdgeDef";
import type { EdgeInfo } from "../shared/generated/EdgeInfo";
import type { EdgeTriggerStatus } from "../pipeline/model/pipeline";
import type { RunState } from "../shared/generated/RunState";
import { formatWhenPill } from "../pipeline/components/editEdgeDerivation";
import { emergentInputs, primaryPort } from "./edgePorts";

/**
 * Projects an edge's runtime trigger status (ADR-0011, #147) from the run state.
 *
 * The daemon does not yet emit per-edge evaluation events, so this is a
 * best-effort projection: an edge is considered to have **fired** once its
 * source node has been evaluated (`completed`) and its target node has been
 * spawned (left `pending`) — or, for End, once End received the port. The `last_value` summarises the edge's `when:`
 * clause; `iter` and `evaluated_at` come from the source node. Returns null
 * before the source has been evaluated, or when there is no active run — the
 * panel then shows its empty state.
 *
 * This status is read ONLY by the edge detail panel; it is never rendered on
 * the canvas.
 */
export function deriveEdgeTrigger(
  runState: RunState | null | undefined,
  edge: EdgeDef,
): EdgeTriggerStatus | null {
  if (!runState) return null;

  const source = runState.nodes[edge.source.node];
  if (!source || source.status !== "completed") return null;

  const fired = targetReached(runState, edge);

  const runEdge = findRunEdge(runState.edges, edge);
  const whenClause = runEdge?.when_clause ?? edge.when ?? null;
  const lastValue =
    whenClause && typeof whenClause === "object"
      ? formatWhenPill(whenClause)
      : edge.else
        ? "else"
        : null;

  return {
    fired,
    last_value: lastValue,
    evaluated_at: source.completed_at,
    iter: source.iter,
  };
}

/** Whether the edge's target took what it carries: a node left `pending`; End,
 *  a marker with no node state, received the port the edge lands on. */
function targetReached(runState: RunState, edge: EdgeDef): boolean {
  const end = runState.end_node;
  if (end && end.id === edge.target.node) {
    // The inputs the edge lands under on End: its target port for one output,
    // each port's own name for several (CONTEXT.md, emergent inputs).
    const inputs = new Set(emergentInputs(edge).map((i) => i.input));
    return end.ports.some((p) => inputs.has(p.port_name) && p.status === "received");
  }
  const target = runState.nodes[edge.target.node];
  return target != null && target.status !== "pending";
}

function findRunEdge(edges: EdgeInfo[], edge: EdgeDef): EdgeInfo | undefined {
  return edges.find(
    (e) =>
      e.source_node === edge.source.node &&
      e.source_port === primaryPort(edge.source) &&
      e.target_node === edge.target.node &&
      e.target_port === edge.target.port,
  );
}
