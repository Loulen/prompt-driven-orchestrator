import { describe, expect, it } from "vitest";
import { describeChange } from "./draftChange";
import type { EditorNodeDef, EditorPipelineDef } from "./pipeline";

function node(id: string, extra: Partial<EditorNodeDef> = {}): EditorNodeDef {
  return { id, name: id, type: "agent", inputs: [], outputs: [], ...extra } as EditorNodeDef;
}

function pipeline(nodes: EditorNodeDef[], extra: Partial<EditorPipelineDef> = {}): EditorPipelineDef {
  return { name: "p", version: "1", variables: {}, nodes, edges: [], ...extra };
}

describe("the words of a change", () => {
  it("names a node added, deleted, moved or renamed", () => {
    const before = pipeline([node("a")]);
    expect(describeChange(before, pipeline([node("a"), node("b")]))).toBe("add node b");
    expect(describeChange(before, pipeline([]))).toBe("delete node a");
    expect(describeChange(before, pipeline([node("a", { view: { x: 1, y: 2 } })]))).toBe("move a");
    expect(describeChange(before, pipeline([node("a", { name: "designer" })]))).toBe("rename a to designer");
  });

  it("names a renamed output", () => {
    const before = pipeline([node("a", { outputs: [{ name: "out" } as never] })]);
    const after = pipeline([node("a", { outputs: [{ name: "spec" } as never] })]);
    expect(describeChange(before, after)).toBe("rename output spec");
  });

  it("names an edge, then a field of the pipeline", () => {
    const edge = { source: { node: "a", port: "o" }, target: { node: "b", port: "i" } };
    expect(describeChange(pipeline([]), pipeline([], { edges: [edge] }))).toBe("add edge");
    expect(describeChange(pipeline([]), pipeline([], { name: "q" }))).toBe("edit pipeline name");
  });

  it("names an edge whose route was reshaped, or whose label was moved", () => {
    const edge = { source: { node: "a", port: "o" }, target: { node: "b", port: "i" } };
    const reshaped = { ...edge, mode: "manual" as const, waypoints: [{ x: 10, y: 20 }] };
    expect(describeChange(pipeline([], { edges: [edge] }), pipeline([], { edges: [reshaped] }))).toBe(
      "reshape edge",
    );
    const labelled = { ...edge, condition_label_pos: { x: 5, y: 5 } };
    expect(describeChange(pipeline([], { edges: [edge] }), pipeline([], { edges: [labelled] }))).toBe(
      "move edge label",
    );
    const conditioned = { ...edge, when: { ok: true } };
    expect(describeChange(pipeline([], { edges: [edge] }), pipeline([], { edges: [conditioned] }))).toBe(
      "edit edge",
    );
  });

  it("has no words for a step that changes nothing", () => {
    // A segment drag writes the same snapped waypoints on many pointer moves:
    // such a step must not rename the gesture's Undo to a generic label.
    const edge = { source: { node: "a", port: "o" }, target: { node: "b", port: "i" }, waypoints: [{ x: 1, y: 2 }] };
    expect(describeChange(pipeline([], { edges: [edge] }), pipeline([], { edges: [{ ...edge }] }))).toBeNull();
  });
});
