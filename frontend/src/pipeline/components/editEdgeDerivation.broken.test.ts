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

describe("deriveEditEdges — broken edges (CORE-88)", () => {
  it("flags the carried ports the source no longer declares, and only those", () => {
    const pipeline: EditorPipelineDef = {
      name: "p",
      version: "1.0",
      variables: {},
      nodes: [node("reviewer", ["verdict"]), node("fixer", [])],
      edges: [
        { source: { node: "reviewer", port: "draft" }, target: { node: "fixer", port: "draft" } },
        { source: { node: "reviewer", port: "verdict" }, target: { node: "fixer", port: "verdict" } },
      ],
    };
    const [broken, sound] = deriveEditEdges(pipeline);
    expect(broken.data?.missingPorts).toEqual(["draft"]);
    expect(sound.data?.missingPorts).toEqual([]);
  });
});
