import { describe, it, expect } from "vitest";
import {
  deriveEditEdges,
  deriveEditNodes,
  formatWhenPill,
  markerReached,
  statusForNode,
} from "./editNodeDerivation";
import type { NodeStatus, NodeType, PipelineDef, RunState, RunStatus } from "../types";

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

describe("markerReached", () => {
  function runWith(status: RunStatus): RunState {
    return { ...makeRunState({}), status };
  }

  it("is true for start/end markers only when the run completed", () => {
    const run = runWith("completed");
    expect(markerReached("start", run)).toBe(true);
    expect(markerReached("end", run)).toBe(true);
  });

  it("is false for non-marker node types even on a completed run", () => {
    const run = runWith("completed");
    const others: NodeType[] = [
      "doc-only",
      "code-mutating",
      "switch",
      "loop",
      "for-each",
      "merge",
    ];
    for (const t of others) expect(markerReached(t, run)).toBe(false);
  });

  it("is false for markers on live / non-success terminal statuses (end keeps non-completed styling)", () => {
    const notReached: RunStatus[] = [
      "running",
      "awaiting_user",
      "paused",
      "failed",
      "halted",
      "archived",
    ];
    for (const status of notReached) {
      expect(markerReached("start", runWith(status))).toBe(false);
      expect(markerReached("end", runWith(status))).toBe(false);
    }
  });

  it("is false when there is no run state (library/template editing)", () => {
    expect(markerReached("start", null)).toBe(false);
    expect(markerReached("end", undefined)).toBe(false);
  });
});

describe("deriveEditNodes — start/end green-on-complete flag (issue #105, inline run view)", () => {
  function makeStartEndPipeline(): PipelineDef {
    return {
      name: "p",
      version: null,
      variables: {},
      nodes: [
        {
          id: "start",
          name: "start",
          type: "start",
          inputs: [],
          outputs: [{ name: "out", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 0, y: 0 },
        },
        {
          id: "work",
          name: "implementer",
          type: "code-mutating",
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "out", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 200, y: 0 },
        },
        {
          id: "end",
          name: "end",
          type: "end",
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [],
          interactive: false,
          view: { x: 400, y: 0 },
        },
      ],
      edges: [],
    };
  }

  function runWith(status: RunStatus): RunState {
    return { ...makeRunState({}), status };
  }

  function reachedById(run: RunState | null) {
    const nodes = deriveEditNodes(makeStartEndPipeline(), run);
    return Object.fromEntries(
      nodes.map((n) => [n.id, (n.data as { reached?: boolean }).reached]),
    );
  }

  it("marks start and end reached on a completed run", () => {
    const r = reachedById(runWith("completed"));
    expect(r.start).toBe(true);
    expect(r.end).toBe(true);
  });

  it("never marks a regular work node reached, even when the run completed", () => {
    expect(reachedById(runWith("completed")).work).toBe(false);
  });

  it("leaves start and end not-reached while the run is still running", () => {
    const r = reachedById(runWith("running"));
    expect(r.start).toBe(false);
    expect(r.end).toBe(false);
  });

  it("does not mark reached for failed or halted runs", () => {
    for (const status of ["failed", "halted"] as RunStatus[]) {
      const r = reachedById(runWith(status));
      expect(r.start).toBe(false);
      expect(r.end).toBe(false);
    }
  });

  it("does not mark reached when editing a template (no run state)", () => {
    const r = reachedById(null);
    expect(r.start).toBe(false);
    expect(r.end).toBe(false);
  });
});

describe("formatWhenPill — condition pill text (ADR-0011)", () => {
  it("renders a single field/op as 'field op value'", () => {
    expect(formatWhenPill({ severity: { eq: "high" } })).toBe("severity = high");
    expect(formatWhenPill({ security: { eq: true } })).toBe("security = true");
    expect(formatWhenPill({ score: { gte: 8 } })).toBe("score >= 8");
  });

  it("renders 'in' / 'not_in' with a bracketed list", () => {
    expect(formatWhenPill({ verdict: { in: ["PASS", "APPROVED"] } })).toBe(
      "verdict in [PASS, APPROVED]",
    );
  });

  it("joins multiple predicates with 'and'", () => {
    expect(
      formatWhenPill({ verdict: { eq: "PASS" }, score: { gte: 8 } }),
    ).toBe("verdict = PASS and score >= 8");
  });
});

describe("deriveEditEdges — condition pills always visible at midpoint (issue #144)", () => {
  function condPipeline(): PipelineDef {
    return {
      name: "cond",
      version: null,
      variables: {},
      nodes: [
        {
          id: "classifier",
          name: "classifier",
          type: "doc-only",
          inputs: [{ name: "task", repeated: false, side: "left" }],
          outputs: [{ name: "triage", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 0, y: 0 },
        },
        {
          id: "hotfix",
          name: "hotfix",
          type: "code-mutating",
          inputs: [{ name: "triage", repeated: false, side: "left" }],
          outputs: [{ name: "patch", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 200, y: 0 },
        },
        {
          id: "backlog",
          name: "backlog",
          type: "doc-only",
          inputs: [{ name: "triage", repeated: false, side: "left" }],
          outputs: [{ name: "note", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 200, y: 200 },
        },
      ],
      edges: [
        {
          source: { node: "classifier", port: "triage" },
          target: { node: "hotfix", port: "triage" },
          when: { severity: { eq: "high" } },
        },
        {
          source: { node: "classifier", port: "triage" },
          target: { node: "backlog", port: "triage" },
          else: true,
        },
      ],
    };
  }

  it("labels a guarded edge with its when: pill", () => {
    const edges = deriveEditEdges(condPipeline());
    const guarded = edges[0];
    expect(guarded.label).toBe("severity = high");
    expect(guarded.data?.isConditional).toBe(true);
    expect(guarded.data?.isElse).toBe(false);
  });

  it("labels an else edge with 'else'", () => {
    const edges = deriveEditEdges(condPipeline());
    const fallback = edges[1];
    expect(fallback.label).toBe("else");
    expect(fallback.data?.isConditional).toBe(true);
    expect(fallback.data?.isElse).toBe(true);
  });

  it("gives unconditional edges no pill", () => {
    const pipeline: PipelineDef = {
      name: "plain",
      version: null,
      variables: {},
      nodes: [
        {
          id: "a",
          name: "a",
          type: "doc-only",
          inputs: [],
          outputs: [{ name: "out", repeated: false, side: "right" }],
          interactive: false,
          view: { x: 0, y: 0 },
        },
        {
          id: "b",
          name: "b",
          type: "doc-only",
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [],
          interactive: false,
          view: { x: 200, y: 0 },
        },
      ],
      edges: [
        {
          source: { node: "a", port: "out" },
          target: { node: "b", port: "in" },
        },
      ],
    };
    const edges = deriveEditEdges(pipeline);
    expect(edges[0].label).toBeUndefined();
    expect(edges[0].data?.isConditional).toBe(false);
  });
});
