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
});
