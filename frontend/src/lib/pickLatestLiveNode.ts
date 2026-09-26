import type { NodeState } from "../shared/generated/NodeState";
import type { RunState } from "../shared/generated/RunState";

// Picks the node a user almost certainly came to watch on a live run: the most
// recently-started `running` node, falling back to the most recently-started
// `awaiting_user` node. Returns null when no live node exists.
export function pickLatestLiveNode(run: RunState): string | null {
  const byStartedDesc = (a: NodeState, b: NodeState) => {
    const ta = a.started_at ? Date.parse(a.started_at) : 0;
    const tb = b.started_at ? Date.parse(b.started_at) : 0;
    return tb - ta;
  };

  const nodes = Object.values(run.nodes);
  const running = nodes
    .filter((n) => n.status === "running")
    .sort(byStartedDesc);
  if (running.length > 0) return running[0].node_id;

  const awaiting = nodes
    .filter((n) => n.status === "awaiting_user")
    .sort(byStartedDesc);
  if (awaiting.length > 0) return awaiting[0].node_id;

  return null;
}
