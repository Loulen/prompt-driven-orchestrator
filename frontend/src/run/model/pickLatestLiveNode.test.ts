import { describe, it, expect } from "vitest";
import { pickLatestLiveNode } from "./pickLatestLiveNode";
import type { NodeState } from "../../shared/generated/NodeState";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunState } from "../../shared/generated/RunState";

function makeNode(
  id: string,
  status: NodeStatus,
  started_at: string | null,
): NodeState {
  return { frontmatter_retries: 0,
    node_id: id,
    status,
    iter: 1,
    started_at,
    completed_at: null,
    failure_reason: null,
    iterations: [],
  };
}

function makeRun(nodes: NodeState[]): RunState {
  return {
    loop_states: {},
    foreach_states: {},
    collection_states: {},
    switch_states: {},
    sandbox: "off",
    sessions_spawned: 0,
    run_id: "r",
    status: "running",
    pipeline_name: "p",
    input: null,
    started_at: null,
    completed_at: null,
    nodes: Object.fromEntries(nodes.map((n) => [n.node_id, n])),
    edges: [],
    node_defs: [],
    start_node: undefined,
    end_node: undefined,
  };
}

describe("pickLatestLiveNode", () => {
  it("returns null when no live node exists", () => {
    const run = makeRun([
      makeNode("a", "completed", "2026-01-01T00:00:00Z"),
      makeNode("b", "pending", null),
      makeNode("c", "failed", "2026-01-01T00:00:00Z"),
    ]);
    expect(pickLatestLiveNode(run)).toBeNull();
  });

  it("returns null on an empty run", () => {
    const run = makeRun([]);
    expect(pickLatestLiveNode(run)).toBeNull();
  });

  it("returns the only running node", () => {
    const run = makeRun([
      makeNode("done", "completed", "2026-01-01T00:00:00Z"),
      makeNode("live", "running", "2026-01-01T00:01:00Z"),
    ]);
    expect(pickLatestLiveNode(run)).toBe("live");
  });

  it("picks the most recently-started running node among concurrent ones", () => {
    const run = makeRun([
      makeNode("older", "running", "2026-01-01T00:00:00Z"),
      makeNode("newer", "running", "2026-01-01T00:05:00Z"),
      makeNode("oldest", "running", "2025-12-31T23:00:00Z"),
    ]);
    expect(pickLatestLiveNode(run)).toBe("newer");
  });

  it("prefers running over awaiting_user even when awaiting_user is newer", () => {
    const run = makeRun([
      makeNode("running-old", "running", "2026-01-01T00:00:00Z"),
      makeNode("awaiting-new", "awaiting_user", "2026-01-01T00:10:00Z"),
    ]);
    expect(pickLatestLiveNode(run)).toBe("running-old");
  });

  it("falls back to the latest awaiting_user when no node is running", () => {
    const run = makeRun([
      makeNode("done", "completed", "2026-01-01T00:00:00Z"),
      makeNode("await-old", "awaiting_user", "2026-01-01T00:00:00Z"),
      makeNode("await-new", "awaiting_user", "2026-01-01T00:10:00Z"),
    ]);
    expect(pickLatestLiveNode(run)).toBe("await-new");
  });

  it("treats a missing started_at as the oldest", () => {
    const run = makeRun([
      makeNode("no-started", "running", null),
      makeNode("started", "running", "2026-01-01T00:00:00Z"),
    ]);
    expect(pickLatestLiveNode(run)).toBe("started");
  });
});
