import type { EdgeDef, EdgeInfo, EdgeTriggerStatus, RunState } from "../types";
import { formatWhenPill } from "../components/editNodeDerivation";

/**
 * Projects an edge's runtime trigger status (ADR-0011, #147) from the run state.
 *
 * The daemon does not yet emit per-edge evaluation events, so this is a
 * best-effort projection: an edge is considered to have **fired** once its
 * source node has been evaluated (`completed`) and its target node has been
 * spawned (left `pending`). The `last_value` summarises the edge's `when:`
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

  const target = runState.nodes[edge.target.node];
  const fired = target != null && target.status !== "pending";

  const runEdge = findRunEdge(runState.edges, edge);
  const whenClause = runEdge?.when_clause ?? edge.when ?? null;
  const lastValue =
    whenClause && typeof whenClause === "object"
      ? formatWhenPill(whenClause as Record<string, unknown>)
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

function findRunEdge(edges: EdgeInfo[], edge: EdgeDef): EdgeInfo | undefined {
  return edges.find(
    (e) =>
      e.source_node === edge.source.node &&
      e.source_port === edge.source.port &&
      e.target_node === edge.target.node &&
      e.target_port === edge.target.port,
  );
}
