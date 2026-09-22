import { describe, it, expect } from "vitest";
import { derivePooledInputs } from "./derivePooledInputs";
import type { NodeDef, PipelineDef } from "../types";

function node(id: string, extra: Partial<NodeDef> = {}): NodeDef {
  return {
    id,
    name: id,
    type: "agent",
    inputs: [],
    outputs: [],
    interactive: false,
    ...extra,
  };
}

function pipeline(nodes: NodeDef[], edges: PipelineDef["edges"]): PipelineDef {
  return { name: "p", version: "1.0", variables: {}, nodes, edges };
}

describe("derivePooledInputs", () => {
  it("derives one input named after the source document from a single incoming edge", () => {
    const p = pipeline(
      [node("reviewer"), node("implementer")],
      [{ source: { node: "reviewer", port: "review" }, target: { node: "implementer", port: "review" } }],
    );

    const inputs = derivePooledInputs(p, "implementer");

    expect(inputs).toEqual([
      { name: "review", repeated: false, sources: [{ nodeId: "reviewer", label: "reviewer", edgeIndex: 0, port: "review", sharedEdge: false }] },
    ]);
  });

  it("pools two same-named incoming edges into one input listing both source nodes", () => {
    const p = pipeline(
      [node("security-reviewer"), node("perf-reviewer"), node("implementer")],
      [
        { source: { node: "security-reviewer", port: "review" }, target: { node: "implementer", port: "review" } },
        { source: { node: "perf-reviewer", port: "review" }, target: { node: "implementer", port: "review" } },
      ],
    );

    const inputs = derivePooledInputs(p, "implementer");

    expect(inputs).toEqual([
      {
        name: "review",
        repeated: false,
        sources: [
          { nodeId: "security-reviewer", label: "security-reviewer", edgeIndex: 0, port: "review", sharedEdge: false },
          { nodeId: "perf-reviewer", label: "perf-reviewer", edgeIndex: 1, port: "review", sharedEdge: false },
        ],
      },
    ]);
  });

  it("keeps distinct-named incoming edges as separate inputs in edge order", () => {
    const p = pipeline(
      [node("planner"), node("debugger"), node("implementer")],
      [
        { source: { node: "planner", port: "task" }, target: { node: "implementer", port: "task" } },
        { source: { node: "debugger", port: "repro_steps" }, target: { node: "implementer", port: "repro_steps" } },
      ],
    );

    const inputs = derivePooledInputs(p, "implementer");

    expect(inputs.map((i) => i.name)).toEqual(["task", "repro_steps"]);
    expect(inputs).toHaveLength(2);
  });

  it("marks the pooled input repeated when any contributing edge sets repeated (read off the edge)", () => {
    const p = pipeline(
      [node("worker"), node("loop-body")],
      [
        { source: { node: "worker", port: "lap" }, target: { node: "loop-body", port: "lap" }, repeated: true },
      ],
    );

    const inputs = derivePooledInputs(p, "loop-body");

    expect(inputs).toEqual([
      { name: "lap", repeated: true, sources: [{ nodeId: "worker", label: "worker", edgeIndex: 0, port: "lap", sharedEdge: false }] },
    ]);
  });

  it("labels a source by its node id when the source node has no name", () => {
    const p = pipeline(
      [node("rv-7", { name: null }), node("implementer")],
      [{ source: { node: "rv-7", port: "review" }, target: { node: "implementer", port: "review" } }],
    );

    const inputs = derivePooledInputs(p, "implementer");

    expect(inputs[0].sources).toEqual([{ nodeId: "rv-7", label: "rv-7", edgeIndex: 0, port: "review", sharedEdge: false }]);
  });

  it("returns an empty list for a node with no incoming edges", () => {
    const p = pipeline([node("orphan")], []);
    expect(derivePooledInputs(p, "orphan")).toEqual([]);
  });

  it("carries the pipeline.edges index on each source, skipping unrelated edges (#339)", () => {
    const p = pipeline(
      [node("a"), node("b"), node("c")],
      [
        { source: { node: "a", port: "out" }, target: { node: "b", port: "out" } }, // unrelated
        { source: { node: "a", port: "in" }, target: { node: "c", port: "in" } },
        { source: { node: "b", port: "in" }, target: { node: "c", port: "in" } },
      ],
    );

    const inputs = derivePooledInputs(p, "c");

    expect(inputs).toHaveLength(1);
    expect(inputs[0].sources.map((s) => s.edgeIndex)).toEqual([1, 2]);
  });

  it("yields a source row with its edgeIndex for a self-edge (#339 self-feed trap)", () => {
    const p = pipeline(
      [node("c")],
      [{ source: { node: "c", port: "in" }, target: { node: "c", port: "in" } }],
    );

    const inputs = derivePooledInputs(p, "c");

    expect(inputs).toEqual([
      { name: "in", repeated: false, sources: [{ nodeId: "c", label: "c", edgeIndex: 0, port: "in", sharedEdge: false }] },
    ]);
  });

  it("gives two same-source same-named edges two sources with distinct indices", () => {
    const p = pipeline(
      [node("a"), node("c")],
      [
        { source: { node: "a", port: "in" }, target: { node: "c", port: "in" } },
        { source: { node: "a", port: "in" }, target: { node: "c", port: "in" } },
      ],
    );

    const inputs = derivePooledInputs(p, "c");

    expect(inputs[0].sources.map((s) => s.edgeIndex)).toEqual([0, 1]);
  });

  // #843 / ADR-0073: one edge, one firing, one emergent input PER CARRIED PORT.
  it("derives one input per carried port of a multi-output edge", () => {
    const p = pipeline(
      [node("design"), node("orch")],
      [
        {
          source: { node: "design", ports: ["out", "spec"] },
          target: { node: "orch", port: "out" },
        },
      ],
    );

    const inputs = derivePooledInputs(p, "orch");

    expect(inputs.map((i) => i.name)).toEqual(["out", "spec"]);
    // Both rows point at the SAME edge, and say so — dropping one must untick
    // its port rather than delete the arrow and take the other input with it.
    expect(inputs.flatMap((i) => i.sources)).toEqual([
      { nodeId: "design", label: "design", edgeIndex: 0, port: "out", sharedEdge: true },
      { nodeId: "design", label: "design", edgeIndex: 0, port: "spec", sharedEdge: true },
    ]);
  });

  it("pools a multi-port input with a same-named single-port edge from elsewhere", () => {
    const p = pipeline(
      [node("design"), node("other"), node("orch")],
      [
        {
          source: { node: "design", ports: ["out", "spec"] },
          target: { node: "orch", port: "out" },
        },
        { source: { node: "other", port: "spec" }, target: { node: "orch", port: "spec" } },
      ],
    );

    const inputs = derivePooledInputs(p, "orch");

    const spec = inputs.find((i) => i.name === "spec")!;
    expect(spec.sources.map((s) => s.nodeId)).toEqual(["design", "other"]);
  });
});
