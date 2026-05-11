import { describe, it, expect } from "vitest";
import { deriveEditNodes, statusForNode } from "./editNodeDerivation";
import type { NodeStatus, PipelineDef, RunState } from "../types";

function makePipeline(): PipelineDef {
  return {
    name: "p",
    version: null,
    variables: {},
    nodes: [
      {
        id: "impl",
        name: "implementer",
        type: "code-mutating",
        inputs: [{ name: "in", repeated: false, side: "left" }],
        outputs: [{ name: "out", repeated: false, side: "right" }],
        interactive: false,
        view: { x: 100, y: 100 },
      },
      {
        id: "sw1",
        name: "switch",
        type: "switch",
        inputs: [{ name: "in", repeated: false, side: "left" }],
        outputs: [
          { name: "branch", repeated: false, side: "right" },
          { name: "default", repeated: false, side: "right" },
        ],
        interactive: false,
        view: { x: 200, y: 100 },
      },
      {
        id: "loop1",
        name: "loop",
        type: "loop",
        inputs: [{ name: "in", repeated: false, side: "left" }],
        outputs: [{ name: "body", repeated: false, side: "right" }],
        interactive: false,
        max_iter: 5,
        view: { x: 300, y: 100 },
      },
      {
        id: "fe1",
        name: "foreach",
        type: "for-each",
        inputs: [{ name: "in", repeated: false, side: "left" }],
        outputs: [{ name: "body", repeated: false, side: "right" }],
        interactive: false,
        view: { x: 400, y: 100 },
      },
      {
        id: "m1",
        name: "merge",
        type: "merge",
        inputs: [{ name: "branches", repeated: true, side: "left" }],
        outputs: [{ name: "merged", repeated: false, side: "right" }],
        interactive: false,
        view: { x: 500, y: 100 },
      },
    ],
    edges: [],
  };
}

function makeRunState(
  statuses: Record<string, NodeStatus>,
  runId = "run-1",
): RunState {
  return {
    run_id: runId,
    pipeline_name: "p",
    status: "running",
    input: null,
    started_at: null,
    completed_at: null,
    nodes: Object.fromEntries(
      Object.entries(statuses).map(([id, status]) => [
        id,
        {
          node_id: id,
          status,
          iter: 1,
          started_at: null,
          completed_at: null,
          failure_reason: null,
          iterations: [],
        },
      ]),
    ),
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
  };
}

describe("statusForNode", () => {
  it("returns the node's live status when present", () => {
    const run = makeRunState({ impl: "running" });
    expect(statusForNode("impl", run)).toBe("running");
  });

  it("defaults to 'pending' when no run state is given", () => {
    expect(statusForNode("impl", null)).toBe("pending");
    expect(statusForNode("impl", undefined)).toBe("pending");
  });

  it("defaults to 'pending' when the node is absent from run.nodes (e.g. a newly added node)", () => {
    const run = makeRunState({ other: "running" });
    expect(statusForNode("impl", run)).toBe("pending");
  });
});

describe("deriveEditNodes — live status wiring (regression: node-card borders ignore run state)", () => {
  it("forwards live status into every node type's data (regular / switch / loop / for-each / merge)", () => {
    const pipeline = makePipeline();
    const run = makeRunState({
      impl: "running",
      sw1: "completed",
      loop1: "awaiting_user",
      fe1: "failed",
      m1: "completed",
    });
    const nodes = deriveEditNodes(pipeline, run);
    const byId = Object.fromEntries(nodes.map((n) => [n.id, n.data]));

    expect((byId.impl as { status: NodeStatus }).status).toBe("running");
    expect((byId.sw1 as { status: NodeStatus }).status).toBe("completed");
    expect((byId.loop1 as { status: NodeStatus }).status).toBe("awaiting_user");
    expect((byId.fe1 as { status: NodeStatus }).status).toBe("failed");
    expect((byId.m1 as { status: NodeStatus }).status).toBe("completed");
  });

  it("defaults every node to 'pending' when no run state is given (template editing)", () => {
    const nodes = deriveEditNodes(makePipeline(), null);
    for (const n of nodes) {
      expect((n.data as { status: NodeStatus }).status).toBe("pending");
    }
  });

  it("uses 'pending' for nodes that exist in the pipeline but not in run.nodes (newly added)", () => {
    const pipeline = makePipeline();
    const run = makeRunState({ impl: "running" }); // sw1/loop1/fe1/m1 absent
    const nodes = deriveEditNodes(pipeline, run);
    const byId = Object.fromEntries(nodes.map((n) => [n.id, n.data]));
    expect((byId.impl as { status: NodeStatus }).status).toBe("running");
    expect((byId.sw1 as { status: NodeStatus }).status).toBe("pending");
    expect((byId.loop1 as { status: NodeStatus }).status).toBe("pending");
    expect((byId.fe1 as { status: NodeStatus }).status).toBe("pending");
    expect((byId.m1 as { status: NodeStatus }).status).toBe("pending");
  });
});
