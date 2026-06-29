import { describe, it, expect } from "vitest";
import {
  collectionFanoutNudges,
  reconcileLoopRegions,
  regionsDestroyedByEdgeRemoval,
} from "./loopRegions";
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

function edge(s: string, sp: string, t: string, tp: string): EdgeDef {
  return { source: { node: s, port: sp }, target: { node: t, port: tp } };
}

describe("reconcileLoopRegions — node deletion repairs the loops: block (#173)", () => {
  it("destroys a bounded region whose last cycle went with the deleted member, ghost id and all", () => {
    // Post-deletion of `rev` from [impl, rev]: rev and its edges (impl->rev,
    // rev->impl) are gone, leaving only start->impl. The region no longer closes
    // a cycle and names a node that no longer exists — it is destroyed, not left
    // as an orphan with a dangling member id.
    const p: PipelineDef = {
      name: "rl",
      variables: {},
      nodes: [plainNode("start"), plainNode("impl")],
      edges: [edge("start", "out", "impl", "task")],
      loops: [{ id: "review_loop", kind: "bounded", members: ["impl", "rev"], max_iter: 3 }],
    };
    expect(reconcileLoopRegions(p)).toEqual([]);
  });

  it("prunes the ghost id but keeps the region when the survivors still close a cycle", () => {
    // [a, b, c] closed by a<->b AND a<->c. Deleting c removes a->c / c->a, but
    // a<->b still cycles, so the region survives with c pruned from members.
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [plainNode("a"), plainNode("b")],
      edges: [edge("a", "out", "b", "in"), edge("b", "out", "a", "in")],
      loops: [{ id: "r", kind: "bounded", members: ["a", "b", "c"], max_iter: 3 }],
    };
    const out = reconcileLoopRegions(p);
    expect(out).toHaveLength(1);
    expect(out[0].members).toEqual(["a", "b"]);
    expect(out[0].max_iter).toBe(3);
  });

  it("keeps a single-member bounded region when the survivor self-loops, pruned to it", () => {
    // The other member is deleted but the survivor carries a self-edge — still a
    // valid (one-member) bounded loop. Kept, with the ghost member pruned.
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [plainNode("worker")],
      edges: [edge("worker", "out", "worker", "again")],
      loops: [{ id: "spin", kind: "bounded", members: ["worker", "rev"], max_iter: 4 }],
    };
    const out = reconcileLoopRegions(p);
    expect(out).toHaveLength(1);
    expect(out[0].members).toEqual(["worker"]);
  });

  it("drops a bounded region left with no present members", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [plainNode("a")],
      edges: [],
      loops: [{ id: "r", kind: "bounded", members: ["gone1", "gone2"], max_iter: 3 }],
    };
    expect(reconcileLoopRegions(p)).toEqual([]);
  });

  it("prunes a collection region's ghost member but keeps it (born by gesture, not a cycle)", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [plainNode("fixer")],
      edges: [],
      loops: [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer", "gone"] }],
    };
    const out = reconcileLoopRegions(p);
    expect(out).toHaveLength(1);
    expect(out[0].members).toEqual(["fixer"]);
  });

  it("drops a collection region emptied of present members", () => {
    const p: PipelineDef = {
      name: "p",
      variables: {},
      nodes: [],
      edges: [],
      loops: [{ id: "per-issue", kind: "collection", over: "issues", members: ["gone"] }],
    };
    expect(reconcileLoopRegions(p)).toEqual([]);
  });

  it("leaves regions untouched when the deleted node was no member (cycle intact)", () => {
    const p = reviewLoop(); // [impl, rev] all present, the cycle is intact
    expect(reconcileLoopRegions(p)).toEqual(p.loops);
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
    expect(nudges[0].message).toContain("fan out over a collection");
    expect(nudges[0].id).toBe("fanout:fixer");
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
