import { describe, it, expect } from "vitest";
import { collectionFanoutNudges, regionsDestroyedByEdgeRemoval } from "./loopRegions";
import type { EdgeDef, NodeDef, PipelineDef } from "../types";

// A bare review loop: start -> impl -> rev, with rev -> impl closing the cycle.
// Edge order: 0 start->impl, 1 impl->rev, 2 rev->impl (the back-edge).
function reviewLoop(): PipelineDef {
  const nodes: NodeDef[] = [
    { id: "start", name: "start", type: "start", inputs: [], outputs: [{ name: "user_prompt", repeated: false, side: "right" }], interactive: false },
    { id: "impl", name: "impl", type: "code-mutating", inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }], interactive: false },
    { id: "rev", name: "rev", type: "doc-only", inputs: [], outputs: [{ name: "review", repeated: false, side: "right" }], interactive: false },
  ];
  const edges: EdgeDef[] = [
    { source: { node: "start", port: "user_prompt" }, target: { node: "impl", port: "task" } },
    { source: { node: "impl", port: "code" }, target: { node: "rev", port: "code" } },
    { source: { node: "rev", port: "review" }, target: { node: "impl", port: "review" } },
  ];
  return {
    name: "rl",
    variables: {},
    nodes,
    edges,
    loops: [{ id: "review_loop", kind: "bounded", members: ["impl", "rev"], max_iter: 3 }],
  };
}

describe("regionsDestroyedByEdgeRemoval (#150)", () => {
  it("deleting the only back-edge destroys the region (last cycle removed)", () => {
    const p = reviewLoop();
    // Edge 2 is the rev -> impl back-edge: the region's last (only) cycle.
    expect(regionsDestroyedByEdgeRemoval(p, 2)).toEqual(["review_loop"]);
  });

  it("deleting a forward edge outside the cycle destroys nothing", () => {
    const p = reviewLoop();
    // Edge 0 is start -> impl: not part of the cycle, so no popup.
    expect(regionsDestroyedByEdgeRemoval(p, 0)).toEqual([]);
  });

  it("deleting a non-last cycle edge keeps a region with two cycles", () => {
    // impl <-> rev AND impl -> rev -> mid -> impl: two cycles close the region.
    const nodes: NodeDef[] = [
      { id: "impl", name: "impl", type: "code-mutating", inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }], interactive: false },
      { id: "rev", name: "rev", type: "doc-only", inputs: [], outputs: [{ name: "review", repeated: false, side: "right" }, { name: "extra", repeated: false, side: "right" }], interactive: false },
      { id: "mid", name: "mid", type: "doc-only", inputs: [], outputs: [{ name: "more", repeated: false, side: "right" }], interactive: false },
    ];
    const edges: EdgeDef[] = [
      { source: { node: "impl", port: "code" }, target: { node: "rev", port: "code" } }, // 0
      { source: { node: "rev", port: "review" }, target: { node: "impl", port: "review" } }, // 1 back-edge A
      { source: { node: "rev", port: "extra" }, target: { node: "mid", port: "extra" } }, // 2
      { source: { node: "mid", port: "more" }, target: { node: "impl", port: "more" } }, // 3 back-edge B
    ];
    const p: PipelineDef = {
      name: "rl2",
      variables: {},
      nodes,
      edges,
      loops: [{ id: "review_loop", kind: "bounded", members: ["impl", "rev", "mid"], max_iter: 3 }],
    };
    // Removing back-edge A (1) still leaves impl->rev->mid->impl: region kept.
    expect(regionsDestroyedByEdgeRemoval(p, 1)).toEqual([]);
  });

  it("never destroys a collection region (no topological cycle to lose)", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [listNode("triage", "issues"), plainNode("fixer")],
      edges: [{ source: { node: "triage", port: "issues" }, target: { node: "fixer", port: "in" } }],
      loops: [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    };
    expect(regionsDestroyedByEdgeRemoval(p, 0)).toEqual([]);
  });
});

function listNode(id: string, port: string): NodeDef {
  return {
    id,
    name: id,
    type: "doc-only",
    inputs: [],
    outputs: [
      {
        name: port,
        repeated: false,
        side: "right",
        frontmatter: { [port]: { type: "list" } },
      },
    ],
    interactive: false,
  };
}

function plainNode(id: string, type: NodeDef["type"] = "code-mutating"): NodeDef {
  return {
    id,
    name: id,
    type,
    inputs: [],
    outputs: [{ name: "out", repeated: false, side: "right" }],
    interactive: false,
  };
}

describe("collectionFanoutNudges (#151)", () => {
  it("nudges when a list-typed output feeds a node not in a collection region", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [listNode("triage", "issues"), plainNode("fixer")],
      edges: [{ source: { node: "triage", port: "issues" }, target: { node: "fixer", port: "in" } }],
    };
    const nudges = collectionFanoutNudges(p);
    expect(nudges).toHaveLength(1);
    expect(nudges[0]).toContain("fan out over a collection");
  });

  it("does not nudge once the target is a member of a collection region (never auto-wraps, but respects an existing one)", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [listNode("triage", "issues"), plainNode("fixer")],
      edges: [{ source: { node: "triage", port: "issues" }, target: { node: "fixer", port: "in" } }],
      loops: [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    };
    expect(collectionFanoutNudges(p)).toHaveLength(0);
  });

  it("does not nudge for a non-list output", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [plainNode("a", "doc-only"), plainNode("b")],
      edges: [{ source: { node: "a", port: "out" }, target: { node: "b", port: "in" } }],
    };
    expect(collectionFanoutNudges(p)).toHaveLength(0);
  });
});
