import { describe, it, expect } from "vitest";
import { edgeConditionFields } from "./edgeFields";
import type { PipelineDef, EdgeDef } from "../types";

function pipeline(): PipelineDef {
  return {
    name: "t",
    version: "1.0",
    variables: { threshold: { type: "int", default: 5 } },
    nodes: [
      {
        id: "reviewer",
        name: "reviewer",
        type: "doc-only",
        inputs: [{ name: "task", repeated: false }],
        outputs: [
          {
            name: "verdict",
            repeated: false,
            frontmatter: {
              verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
              is_blocking: { type: "bool" },
            },
          },
          { name: "plain", repeated: false },
        ],
        interactive: false,
        view: { x: 0, y: 0 },
      },
      {
        id: "impl",
        name: "impl",
        type: "code-mutating",
        inputs: [{ name: "review", repeated: false }],
        outputs: [{ name: "diff", repeated: false }],
        interactive: false,
        view: { x: 200, y: 0 },
      },
    ],
    edges: [],
  };
}

const guardedEdge: EdgeDef = {
  source: { node: "reviewer", port: "verdict" },
  target: { node: "impl", port: "review" },
};

describe("edgeConditionFields", () => {
  it("exposes the source port's frontmatter fields with their declarations", () => {
    const fields = edgeConditionFields(pipeline(), guardedEdge);
    const verdict = fields.find((f) => f.name === "verdict");
    const blocking = fields.find((f) => f.name === "is_blocking");
    expect(verdict?.decl?.type).toBe("enum");
    expect(verdict?.decl?.allowed).toEqual(["PASS", "FAIL"]);
    expect(blocking?.decl?.type).toBe("bool");
  });

  it("always offers iter as a region-counter field", () => {
    const fields = edgeConditionFields(pipeline(), guardedEdge);
    const iter = fields.find((f) => f.name === "iter");
    expect(iter).toBeDefined();
    expect(iter?.isIter).toBe(true);
  });

  it("offers pipeline variables prefixed with $", () => {
    const fields = edgeConditionFields(pipeline(), guardedEdge);
    expect(fields.some((f) => f.name === "$threshold")).toBe(true);
  });

  it("offers only iter and variables when the source port has no frontmatter", () => {
    const edge: EdgeDef = {
      source: { node: "reviewer", port: "plain" },
      target: { node: "impl", port: "review" },
    };
    const fields = edgeConditionFields(pipeline(), edge);
    expect(fields.some((f) => f.name === "iter")).toBe(true);
    expect(fields.some((f) => f.name === "$threshold")).toBe(true);
    // No frontmatter on `plain`, so no schema fields.
    expect(fields.some((f) => f.name === "verdict")).toBe(false);
  });
});

describe("isBoolField helper", () => {
  it("detects a bool-typed field", async () => {
    const { isBoolField } = await import("./edgeFields");
    const fields = edgeConditionFields(pipeline(), guardedEdge);
    expect(isBoolField(fields, "is_blocking")).toBe(true);
    expect(isBoolField(fields, "verdict")).toBe(false);
    expect(isBoolField(fields, "iter")).toBe(false);
  });
});
