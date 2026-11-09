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

  it("names a note or a loop added or deleted, as an edge", () => {
    const note = { id: "n1", content: "", view: { x: 0, y: 0 } };
    const noted = pipeline([], { notes: [note] });
    expect(describeChange(pipeline([]), noted)).toBe("add note");
    expect(describeChange(noted, pipeline([], { notes: [] }))).toBe("delete note");
    expect(describeChange(noted, pipeline([], { notes: [{ ...note, content: "x" }] }))).toBe("edit note");
    const loop = { id: "loop", kind: "bounded", members: ["a"], max_iter: 3 };
    const looped = pipeline([], { loops: [loop] } as never);
    expect(describeChange(pipeline([]), looped)).toBe("add loop");
    expect(describeChange(looped, pipeline([], { loops: [] } as never))).toBe("delete loop");
    expect(describeChange(looped, pipeline([], { loops: [{ ...loop, max_iter: 4 }] } as never))).toBe("edit loop");
  });
});
