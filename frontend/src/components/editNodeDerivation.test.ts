import { describe, it, expect } from "vitest";
import { deriveEditEdges, runReachedEnd } from "./editNodeDerivation";
import type { NodeDef, NodeType, PipelineDef, RunStatus } from "../types";

describe("runReachedEnd", () => {
  it("is true only when the run completed successfully", () => {
    expect(runReachedEnd("completed")).toBe(true);
  });

  it("is false for live and non-success terminal statuses", () => {
    const notReached: RunStatus[] = [
      "running",
      "awaiting_user",
      "paused",
      "failed",
      "halted",
      "archived",
    ];
    for (const status of notReached) {
      expect(runReachedEnd(status)).toBe(false);
    }
  });
});

describe("deriveEditEdges targetHandle anchoring (#149)", () => {
  function node(id: string, type: NodeType, inputs: string[], outputs: string[]): NodeDef {
    return {
      id,
      name: id,
      type,
      inputs: inputs.map((name) => ({ name, repeated: false, side: "left" as const })),
      outputs: outputs.map((name) => ({ name, repeated: false, side: "right" as const })),
      interactive: false,
    };
  }

  function pipeline(nodes: NodeDef[], edges: PipelineDef["edges"]): PipelineDef {
    return { name: "p", variables: {}, nodes, edges };
  }

  it("nulls the targetHandle for an emergent input on a regular node so the arrow binds to the body", () => {
    // After migration a regular node declares NO inputs; its body handle is
    // id-less. The edge must target `null` or xyflow drops it (error 008).
    const p = pipeline(
      [node("src", "doc-only", [], ["plan"]), node("dst", "code-mutating", [], ["code"])],
      [{ source: { node: "src", port: "plan" }, target: { node: "dst", port: "plan" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBeNull();
    expect(edges[0].sourceHandle).toBe("plan");
  });

  it("keeps the declared port for the End node (it retains a `result` input handle)", () => {
    const p = pipeline(
      [node("src", "doc-only", [], ["plan"]), node("end", "end", ["result"], [])],
      [{ source: { node: "src", port: "plan" }, target: { node: "end", port: "result" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("result");
  });

  it("keeps the declared port for structural nodes (merge)", () => {
    const p = pipeline(
      [node("src", "doc-only", [], ["plan"]), node("m", "merge", ["branches"], ["merged"])],
      [{ source: { node: "src", port: "plan" }, target: { node: "m", port: "branches" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("branches");
  });

  it("nulls the targetHandle when two same-named edges pool into one body input", () => {
    const p = pipeline(
      [
        node("a", "doc-only", [], ["plan"]),
        node("b", "doc-only", [], ["plan"]),
        node("dst", "code-mutating", [], ["code"]),
      ],
      [
        { source: { node: "a", port: "plan" }, target: { node: "dst", port: "plan" } },
        { source: { node: "b", port: "plan" }, target: { node: "dst", port: "plan" } },
      ],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBeNull();
    expect(edges[1].targetHandle).toBeNull();
  });
});
