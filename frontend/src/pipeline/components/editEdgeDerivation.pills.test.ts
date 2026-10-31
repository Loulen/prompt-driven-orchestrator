import { describe, it, expect } from "vitest";
import { deriveEditEdges } from "./editEdgeDerivation";
import type { EditorNodeDef, EditorPipelineDef } from "../model/pipeline";

const node = (id: string, outputs: string[]): EditorNodeDef => ({
  id,
  name: id,
  type: "agent",
  inputs: [],
  outputs: outputs.map((name) => ({ name })),
  interactive: false,
  view: { x: 0, y: 0 },
});

describe("deriveEditEdges — pinned condition pills", () => {
  it("hands each edge the pills pinned on the OTHER edges", () => {
    const pipeline: EditorPipelineDef = {
      name: "p",
      version: "1.0",
      variables: {},
      nodes: [node("tester", ["out"]), node("ship", []), node("fix", [])],
      edges: [
        {
          source: { node: "tester", port: "out" },
          target: { node: "ship", port: "in" },
          when: { Verdict: { eq: "Pass" } },
          condition_label_pos: { x: 399, y: 346 },
        },
        { source: { node: "tester", port: "out" }, target: { node: "fix", port: "in" }, else: true },
      ],
    };
    const [pinned, auto] = deriveEditEdges(pipeline);
    expect(pinned.data?.otherPinnedPills).toEqual([]);
    expect(auto.data?.otherPinnedPills).toEqual([{ x: 399, y: 346, label: pinned.data?.label }]);
  });
});
