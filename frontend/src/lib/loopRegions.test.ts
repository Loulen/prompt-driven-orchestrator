import { describe, it, expect } from "vitest";
import { collectionFanoutNudges } from "./loopRegions";
import type { NodeDef, PipelineDef } from "../types";

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
