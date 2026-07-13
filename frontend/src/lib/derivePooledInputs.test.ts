import { describe, it, expect } from "vitest";
import { derivePooledInputs } from "./derivePooledInputs";
import type { NodeDef, PipelineDef } from "../types";

function node(id: string, extra: Partial<NodeDef> = {}): NodeDef {
  return {
    id,
    name: id,
    type: "doc-only",
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
      { name: "review", repeated: false, sources: [{ nodeId: "reviewer", label: "reviewer", edgeIndex: 0 }] },
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
          { nodeId: "security-reviewer", label: "security-reviewer", edgeIndex: 0 },
          { nodeId: "perf-reviewer", label: "perf-reviewer", edgeIndex: 1 },
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
      { name: "lap", repeated: true, sources: [{ nodeId: "worker", label: "worker", edgeIndex: 0 }] },
    ]);
  });

  it("labels a source by its node id when the source node has no name", () => {
    const p = pipeline(
      [node("rv-7", { name: null }), node("implementer")],
      [{ source: { node: "rv-7", port: "review" }, target: { node: "implementer", port: "review" } }],
    );

    const inputs = derivePooledInputs(p, "implementer");

    expect(inputs[0].sources).toEqual([{ nodeId: "rv-7", label: "rv-7", edgeIndex: 0 }]);
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
      { name: "in", repeated: false, sources: [{ nodeId: "c", label: "c", edgeIndex: 0 }] },
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
});
