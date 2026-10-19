import { describe, expect, it } from "vitest";
import { orderedLike } from "./draftForm";
import type { Json } from "./draftDelta";

describe("orderedLike", () => {
  it("keeps the key order the tab held, the new keys after, sorted", () => {
    const shown: Json = {
      name: "p",
      nodes: [{ id: "a", view: { x: 1, y: 2 } }],
      edges: [{ source: { node: "a", port: "out" }, target: { node: "b", port: "in" } }],
    };
    const draft: Json = {
      edges: [{ target: { port: "in", node: "b" }, source: { port: "out", node: "a" } }],
      nodes: [
        { id: "c", view: { y: 5, x: 4 } },
        { view: { y: 3, x: 2 }, id: "a", name: "A" },
      ],
      name: "p",
    };
    expect(JSON.stringify(orderedLike(draft, shown))).toBe(
      JSON.stringify({
        name: "p",
        nodes: [
          { id: "c", view: { x: 4, y: 5 } },
          { id: "a", view: { x: 2, y: 3 }, name: "A" },
        ],
        edges: [{ source: { node: "a", port: "out" }, target: { node: "b", port: "in" } }],
      }),
    );
  });
});
