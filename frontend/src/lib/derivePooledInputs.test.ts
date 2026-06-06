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
      { name: "review", repeated: false, sources: [{ nodeId: "reviewer", label: "reviewer" }] },
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
          { nodeId: "security-reviewer", label: "security-reviewer" },
          { nodeId: "perf-reviewer", label: "perf-reviewer" },
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
      { name: "lap", repeated: true, sources: [{ nodeId: "worker", label: "worker" }] },
    ]);
  });

  it("labels a source by its node id when the source node has no name", () => {
    const p = pipeline(
      [node("rv-7", { name: null }), node("implementer")],
      [{ source: { node: "rv-7", port: "review" }, target: { node: "implementer", port: "review" } }],
    );

    const inputs = derivePooledInputs(p, "implementer");

    expect(inputs[0].sources).toEqual([{ nodeId: "rv-7", label: "rv-7" }]);
  });

  it("returns an empty list for a node with no incoming edges", () => {
    const p = pipeline([node("orphan")], []);
    expect(derivePooledInputs(p, "orphan")).toEqual([]);
  });
});
