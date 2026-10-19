import { describe, expect, it } from "vitest";
import * as Y from "yjs";
import {
  applyObject,
  applyPrompts,
  canonical,
  diffText,
  elementKey,
  promptsOf,
  shiftCaret,
  type Json,
} from "./draftDelta";

type Obj = { [key: string]: Json };

const base: Obj = {
  name: "p",
  nodes: [
    { id: "a", name: "A", view: { x: 100, y: 40 }, outputs: [{ name: "spec" }] },
    { id: "b", name: "B", view: { x: 100, y: 200 } },
  ],
  edges: [{ source: { node: "a", port: "spec" }, target: { node: "b", port: "spec" } }],
};

function docWith(pipeline: Obj, prompts: Record<string, string> = {}): Y.Doc {
  const doc = new Y.Doc();
  doc.transact(() => {
    applyObject(doc.getMap("pipeline"), {}, pipeline);
    applyPrompts(doc.getMap("prompts"), {}, prompts);
  });
  return doc;
}

function copyOf(doc: Y.Doc): Y.Doc {
  const copy = new Y.Doc();
  Y.applyUpdate(copy, Y.encodeStateAsUpdate(doc));
  return copy;
}

function edit(doc: Y.Doc, before: Obj, after: Obj): Uint8Array {
  const vector = Y.encodeStateVector(doc);
  doc.transact(() => applyObject(doc.getMap("pipeline"), before, after));
  return Y.encodeStateAsUpdate(doc, vector);
}

function clone(value: Obj): Obj {
  return JSON.parse(JSON.stringify(value)) as Obj;
}

describe("the delta of a shared draft", () => {
  it("brings the document to the new state", () => {
    const after = clone(base);
    after.name = "q";
    (after.nodes as Obj[]).reverse();
    (after.nodes as Obj[]).push({ id: "c" });
    after.edges = [{ source: { node: "a", port: "spec" }, target: { node: "c", port: "x" }, else: true }];
    const doc = docWith(base);
    edit(doc, base, after);
    expect(canonical(doc.getMap("pipeline").toJSON() as Json)).toBe(canonical(after));
  });

  it("keeps two fields changed at once on the same node", () => {
    const first = docWith(base);
    const second = copyOf(first);
    const renamed = clone(base);
    (renamed.nodes as Obj[])[0].name = "Designer";
    const moved = clone(base);
    (moved.nodes as Obj[])[0].view = { x: 300, y: 40 };
    const one = edit(first, base, renamed);
    const two = edit(second, base, moved);
    Y.applyUpdate(first, two);
    Y.applyUpdate(second, one);
    const node = (first.getMap("pipeline").toJSON().nodes as Obj[])[0];
    expect(node.name).toBe("Designer");
    expect(node.view).toEqual({ x: 300, y: 40 });
    expect(second.getMap("pipeline").toJSON()).toEqual(first.getMap("pipeline").toJSON());
  });

  it("converges when the same field changes at once", () => {
    const first = docWith(base);
    const second = copyOf(first);
    const one = edit(first, base, { ...base, name: "mine" });
    const two = edit(second, base, { ...base, name: "theirs" });
    Y.applyUpdate(first, two);
    Y.applyUpdate(second, one);
    expect(first.getMap("pipeline").get("name")).toBe(second.getMap("pipeline").get("name"));
  });

  it("finds a node by its id after someone else moved it in the list", () => {
    const doc = docWith(base);
    const theirs = clone(base);
    (theirs.nodes as Obj[]).unshift({ id: "z" });
    edit(doc, base, theirs);
    const mine = clone(base);
    (mine.nodes as Obj[])[1].name = "B2";
    edit(doc, base, mine);
    const nodes = doc.getMap("pipeline").toJSON().nodes as Obj[];
    expect(nodes.map((n) => n.id)).toEqual(["z", "a", "b"]);
    expect(nodes[2].name).toBe("B2");
  });

  it("knows an element by its id, else by its whole value", () => {
    expect(elementKey({ id: "a", x: 1 })).toBe("id:a");
    expect(elementKey({ b: 1, a: 2 })).toBe(elementKey({ a: 2, b: 1 }));
  });
});

describe("the prompts of a shared draft", () => {
  it("keeps the text two people type at once in one prompt", () => {
    const first = docWith(base, { a: "Read the task." });
    const second = copyOf(first);
    const vector = Y.encodeStateVector(first);
    first.transact(() => applyPrompts(first.getMap("prompts"), { a: "Read the task." }, { a: "Please read the task." }));
    second.transact(() =>
      applyPrompts(second.getMap("prompts"), { a: "Read the task." }, { a: "Read the task. Be short." }),
    );
    Y.applyUpdate(second, Y.encodeStateAsUpdate(first, vector));
    Y.applyUpdate(first, Y.encodeStateAsUpdate(second, vector));
    expect(promptsOf(first.getMap("prompts")).a).toBe("Please read the task. Be short.");
    expect(promptsOf(second.getMap("prompts"))).toEqual(promptsOf(first.getMap("prompts")));
  });

  it("changes a text between its common prefix and suffix", () => {
    expect(diffText("abc", "abc")).toBeNull();
    expect(diffText("Keep it short", "Keep it very short")).toEqual({ index: 8, remove: 0, insert: "very " });
    expect(diffText("😀", "😁")).toEqual({ index: 0, remove: 2, insert: "😁" });
  });

  it("keeps a caret in its place when text lands before it", () => {
    expect(shiftCaret(5, "hello world", "hey hello world")).toBe(9);
    expect(shiftCaret(2, "hello world", "hello big world")).toBe(2);
    expect(shiftCaret(8, "hello world", "hello")).toBe(5);
  });
});
