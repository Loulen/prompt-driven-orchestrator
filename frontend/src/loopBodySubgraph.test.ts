import { describe, it, expect } from "vitest";
import { computeBodySubgraph } from "./loopBodySubgraph";
import type { EdgeInfo, NodeDefInfo } from "./types";

function edge(
  srcNode: string,
  srcPort: string,
  tgtNode: string,
  tgtPort: string,
): EdgeInfo {
  return {
    source_node: srcNode,
    source_port: srcPort,
    target_node: tgtNode,
    target_port: tgtPort,
  };
}

function nodeDef(id: string, nodeType: string): NodeDefInfo {
  return {
    id,
    node_type: nodeType as NodeDefInfo["node_type"],
    view_x: null,
    view_y: null,
    inputs: [],
    outputs: [],
  };
}

describe("computeBodySubgraph", () => {
  it("returns body nodes for a linear chain", () => {
    const edges = [
      edge("loop1", "body", "a", "in"),
      edge("a", "out", "b", "in"),
      edge("b", "out", "sw", "in"),
      edge("sw", "pass", "loop1", "break"),
    ];
    const defs = [
      nodeDef("loop1", "loop"),
      nodeDef("a", "doc-only"),
      nodeDef("b", "doc-only"),
      nodeDef("sw", "doc-only"),
    ];

    const body = computeBodySubgraph(edges, defs, "loop1");
    expect(body).toEqual(new Set(["a", "b", "sw"]));
  });

  it("includes all downstream branches that stay in the body", () => {
    const edges = [
      edge("loop1", "body", "impl", "in"),
      edge("impl", "out", "reviewer", "in"),
      edge("reviewer", "review", "sw", "in"),
      edge("sw", "pass", "loop1", "break"),
      edge("sw", "default", "impl", "in"),
    ];
    const defs = [
      nodeDef("loop1", "loop"),
      nodeDef("impl", "code-mutating"),
      nodeDef("reviewer", "doc-only"),
      nodeDef("sw", "doc-only"),
    ];

    const body = computeBodySubgraph(edges, defs, "loop1");
    expect(body).toEqual(new Set(["impl", "reviewer", "sw"]));
  });

  it("treats nested loops as opaque — excludes inner body nodes", () => {
    const edges = [
      edge("outer", "body", "inner", "in"),
      edge("inner", "body", "inner_worker", "in"),
      edge("inner_worker", "out", "inner", "break"),
      edge("inner", "done", "outer", "break"),
    ];
    const defs = [
      nodeDef("outer", "loop"),
      nodeDef("inner", "loop"),
      nodeDef("inner_worker", "doc-only"),
    ];

    const body = computeBodySubgraph(edges, defs, "outer");
    expect(body).toEqual(new Set(["inner"]));
    expect(body.has("inner_worker")).toBe(false);
  });

  it("returns empty set when body port is unwired", () => {
    const defs = [nodeDef("loop1", "loop")];
    const body = computeBodySubgraph([], defs, "loop1");
    expect(body.size).toBe(0);
  });
});
