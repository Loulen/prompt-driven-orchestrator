import type { EdgeInfo, NodeDefInfo } from "./types";

export function computeBodySubgraph(
  edges: EdgeInfo[],
  nodeDefs: NodeDefInfo[],
  loopNodeId: string,
): Set<string> {
  const bodyTargets = edges
    .filter((e) => e.source_node === loopNodeId && e.source_port === "body")
    .map((e) => e.target_node);

  if (bodyTargets.length === 0) return new Set();

  const body = new Set<string>();
  const queue = [...bodyTargets];

  while (queue.length > 0) {
    const current = queue.pop()!;
    if (current === loopNodeId) continue;

    const currentDef = nodeDefs.find((n) => n.id === current);
    if (currentDef?.node_type === "loop" || currentDef?.node_type === "for-each") {
      body.add(current);
      continue;
    }

    if (body.has(current)) continue;
    body.add(current);

    for (const edge of edges) {
      if (edge.source_node !== current) continue;
      if (edge.target_node === loopNodeId) continue;
      if (!body.has(edge.target_node)) {
        queue.push(edge.target_node);
      }
    }
  }

  return body;
}
