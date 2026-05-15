import { describe, it, expect } from "vitest";
import {
  withUpdatedNodeView,
  canvasToYamlX,
  START_NODE_OFFSET_X_PX,
} from "./dagCanvasUtils";
import type { RunState, NodeDefInfo, EdgeInfo, PortBrief, PipelineDef } from "../types";

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

describe("withUpdatedNodeView", () => {
  function makePipeline(): PipelineDef {
    return {
      name: "p",
      version: null,
      variables: {},
      nodes: [
        {
          id: "a",
          name: "a",
          type: "doc-only",
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "out", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 100, y: 100 },
        },
        {
          id: "b",
          name: "b",
          type: "doc-only",
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "out", repeated: false, side: "right" }],
          interactive: false,
          view: null,
        },
      ],
      edges: [],
    };
  }

  it("updates view of a known node and rounds coordinates", () => {
    const updated = withUpdatedNodeView(makePipeline(), "a", 250.6, 80.3);
    expect(updated).not.toBeNull();
    expect(updated!.nodes[0].view).toEqual({ x: 251, y: 80 });
    // other node untouched
    expect(updated!.nodes[1].view).toBeNull();
  });

  it("sets view on a node without one", () => {
    const updated = withUpdatedNodeView(makePipeline(), "b", 320, 240);
    expect(updated!.nodes[1].view).toEqual({ x: 320, y: 240 });
  });

  it("returns null if node id is unknown — drag is a no-op", () => {
    const updated = withUpdatedNodeView(makePipeline(), "ghost", 0, 0);
    expect(updated).toBeNull();
  });

  it("returns a new pipeline object (immutable)", () => {
    const original = makePipeline();
    const updated = withUpdatedNodeView(original, "a", 200, 200);
    expect(updated).not.toBe(original);
    expect(updated!.nodes).not.toBe(original.nodes);
    expect(original.nodes[0].view).toEqual({ x: 100, y: 100 });
  });
});

describe("canvasToYamlX (drag persistence offset)", () => {
  it("subtracts START_NODE_OFFSET_X_PX for regular nodes (pipeline / loop / switch)", () => {
    expect(canvasToYamlX("pipeline", 680)).toBe(680 - START_NODE_OFFSET_X_PX);
    expect(canvasToYamlX("loopRun", 680)).toBe(680 - START_NODE_OFFSET_X_PX);
    expect(canvasToYamlX("switchRun", 680)).toBe(680 - START_NODE_OFFSET_X_PX);
  });

  it("returns canvas X unchanged for start and end nodes", () => {
    expect(canvasToYamlX("start", 50)).toBe(50);
    expect(canvasToYamlX("end", 1430)).toBe(1430);
  });

  it("round-trips with deriveNodes offset (drag-stop with no movement persists same view_x)", () => {
    // Simulate a regular node at YAML view_x = 500 → deriveNodes places it at canvas x = 680.
    // A zero-delta drag should persist view_x = 500, not 680 (regression: was 680 before fix).
    const yamlX = 500;
    const canvasX = yamlX + START_NODE_OFFSET_X_PX;
    expect(canvasToYamlX("pipeline", canvasX)).toBe(yamlX);
  });
});
