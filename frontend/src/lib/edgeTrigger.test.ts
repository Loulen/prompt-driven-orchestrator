import { describe, it, expect } from "vitest";
import { deriveEdgeTrigger } from "./edgeTrigger";
import type { EdgeDef, RunState, EdgeInfo } from "../types";

const edge: EdgeDef = {
  source: { node: "reviewer", port: "verdict" },
  target: { node: "impl", port: "review" },
  when: { verdict: { eq: "FAIL" } },
};

function runState(overrides: Partial<RunState> = {}): RunState {
  const base: RunState = {
    run_id: "r1",
    status: "running",
    pipeline_name: "p",
    input: null,
    started_at: null,
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
  };
  return { ...base, ...overrides };
}

describe("deriveEdgeTrigger", () => {
  it("returns null when there is no run", () => {
    expect(deriveEdgeTrigger(null, edge)).toBeNull();
  });

  it("returns null when the source node has not been evaluated yet", () => {
    const rs = runState({
      nodes: { reviewer: nodeState("reviewer", "pending"), impl: nodeState("impl", "pending") },
    });
    expect(deriveEdgeTrigger(rs, edge)).toBeNull();
  });

  it("reports fired when the source completed and the target was spawned", () => {
    const rs = runState({
      nodes: {
        reviewer: nodeState("reviewer", "completed", 2),
        impl: nodeState("impl", "running"),
      },
    });
    const t = deriveEdgeTrigger(rs, edge)!;
    expect(t.fired).toBe(true);
    expect(t.iter).toBe(2);
  });

  it("reports not fired when the source completed but the target stayed pending", () => {
    const rs = runState({
      nodes: {
        reviewer: nodeState("reviewer", "completed", 1),
        impl: nodeState("impl", "pending"),
      },
    });
    const t = deriveEdgeTrigger(rs, edge)!;
    expect(t.fired).toBe(false);
  });

  it("carries the when_clause from the run edge as the last value summary", () => {
    const runEdge: EdgeInfo = {
      source_node: "reviewer",
      source_port: "verdict",
      target_node: "impl",
      target_port: "review",
      when_clause: { verdict: { eq: "FAIL" } },
    };
    const rs = runState({
      nodes: {
        reviewer: nodeState("reviewer", "completed", 1),
        impl: nodeState("impl", "running"),
      },
      edges: [runEdge],
    });
    const t = deriveEdgeTrigger(rs, edge)!;
    expect(t.last_value).toContain("verdict");
  });
});

function nodeState(id: string, status: string, iter = 0) {
  return {
    node_id: id,
    status: status as never,
    iter,
    started_at: null,
    completed_at: null,
    failure_reason: null,
    iterations: [],
  };
}
