import { describe, it, expect } from "vitest";
import type { RunState, NodeDefInfo, EdgeInfo, PortBrief } from "../types";

function makeRunState(overrides?: Partial<RunState>): RunState {
  return {
    run_id: "run-1",
    pipeline_name: "test",
    status: "running",
    input: null,
    started_at: null,
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
    ...overrides,
  };
}

function makeNodeDef(
  id: string,
  node_type: "doc-only" | "code-mutating" | "start" | "end",
  inputs: PortBrief[] = [],
  outputs: PortBrief[] = [],
): NodeDefInfo {
  return { id, name: id, node_type, view_x: 200, view_y: 100, inputs, outputs };
}

function makeEdge(
  source_node: string,
  source_port: string,
  target_node: string,
  target_port: string,
): EdgeInfo {
  return { source_node, source_port, target_node, target_port };
}

describe("DagCanvas data derivation", () => {
  it("Start node data includes outputs from node_defs", () => {
    const startOutputs: PortBrief[] = [{ name: "user_prompt", side: "right" }];
    const run = makeRunState({
      node_defs: [
        makeNodeDef("start", "start", [], startOutputs),
        makeNodeDef("planner", "doc-only", [{ name: "in", side: "left" }], [{ name: "out", side: "right" }]),
      ],
      start_node: { input_path: "/input.md", started_at: "t0", target_node_ids: ["planner"] },
      edges: [makeEdge("start", "user_prompt", "planner", "in")],
    });

    const startDef = run.node_defs!.find((d) => d.node_type === "start");
    expect(startDef).toBeDefined();
    expect(startDef!.outputs).toEqual([{ name: "user_prompt", side: "right" }]);
  });

  it("End node data includes inputs from node_defs", () => {
    const endInputs: PortBrief[] = [{ name: "result", side: "left" }];
    const run = makeRunState({
      node_defs: [
        makeNodeDef("planner", "doc-only", [{ name: "in", side: "left" }], [{ name: "out", side: "right" }]),
        makeNodeDef("end", "end", endInputs, []),
      ],
      edges: [makeEdge("planner", "out", "end", "result")],
    });

    const endDef = run.node_defs!.find((d) => d.node_type === "end");
    expect(endDef).toBeDefined();
    expect(endDef!.inputs).toEqual([{ name: "result", side: "left" }]);
  });

  it("edge to End preserves targetHandle (not null)", () => {
    const edge = makeEdge("planner", "out", "end", "result");
    const targetHandle = edge.target_port || null;
    expect(targetHandle).toBe("result");
  });

  it("edge from Start preserves sourceHandle", () => {
    const edge = makeEdge("start", "user_prompt", "planner", "in");
    const sourceHandle = edge.source_port || null;
    expect(sourceHandle).toBe("user_prompt");
  });
});
