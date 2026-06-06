import { describe, it, expect } from "vitest";
import { deriveEditEdges, deriveEditNodes, deriveLoopRegions, runReachedEnd } from "./editNodeDerivation";
import type { LoopRegion, NodeDef, NodeType, PipelineDef, RunStatus } from "../types";

describe("runReachedEnd", () => {
  it("is true only when the run completed successfully", () => {
    expect(runReachedEnd("completed")).toBe(true);
  });

  it("is false for live and non-success terminal statuses", () => {
    const notReached: RunStatus[] = [
      "running",
      "awaiting_user",
      "paused",
      "failed",
      "halted",
      "archived",
    ];
    for (const status of notReached) {
      expect(runReachedEnd(status)).toBe(false);
    }
  });
});

describe("deriveEditEdges targetHandle anchoring (#149)", () => {
  function node(id: string, type: NodeType, inputs: string[], outputs: string[]): NodeDef {
    return {
      id,
      name: id,
      type,
      inputs: inputs.map((name) => ({ name, repeated: false, side: "left" as const })),
      outputs: outputs.map((name) => ({ name, repeated: false, side: "right" as const })),
      interactive: false,
    };
  }

  function pipeline(nodes: NodeDef[], edges: PipelineDef["edges"]): PipelineDef {
    return { name: "p", variables: {}, nodes, edges };
  }

  it("binds an emergent incoming edge to a side body handle, defaulting to left (#149/#168)", () => {
    // After migration a regular node declares NO inputs; its body renders one
    // id'd target handle per side (#168). An un-anchored edge binds to the left
    // handle (legacy anchoring) — a real rendered handle, so xyflow keeps the
    // edge (no error 008).
    const p = pipeline(
      [node("src", "doc-only", [], ["plan"]), node("dst", "code-mutating", [], ["code"])],
      [{ source: { node: "src", port: "plan" }, target: { node: "dst", port: "plan" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("__anchor:left");
    expect(edges[0].sourceHandle).toBe("plan");
  });

  it("keeps the declared port for the End node (it retains a `result` input handle)", () => {
    const p = pipeline(
      [node("src", "doc-only", [], ["plan"]), node("end", "end", ["result"], [])],
      [{ source: { node: "src", port: "plan" }, target: { node: "end", port: "result" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("result");
  });

  it("keeps the declared port for structural nodes (merge)", () => {
    const p = pipeline(
      [node("src", "doc-only", [], ["plan"]), node("m", "merge", ["branches"], ["merged"])],
      [{ source: { node: "src", port: "plan" }, target: { node: "m", port: "branches" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("branches");
  });

  it("binds both same-named edges to a side body handle when they pool into one body input (default left)", () => {
    const p = pipeline(
      [
        node("a", "doc-only", [], ["plan"]),
        node("b", "doc-only", [], ["plan"]),
        node("dst", "code-mutating", [], ["code"]),
      ],
      [
        { source: { node: "a", port: "plan" }, target: { node: "dst", port: "plan" } },
        { source: { node: "b", port: "plan" }, target: { node: "dst", port: "plan" } },
      ],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("__anchor:left");
    expect(edges[1].targetHandle).toBe("__anchor:left");
  });

  it("anchors each pooled edge on its own persisted side (#168)", () => {
    // Two edges pool into the same emergent body but may arrive from different
    // sides; each keeps its own anchor.
    const p = pipeline(
      [
        node("a", "doc-only", [], ["plan"]),
        node("b", "doc-only", [], ["plan"]),
        node("dst", "code-mutating", [], ["code"]),
      ],
      [
        { source: { node: "a", port: "plan" }, target: { node: "dst", port: "plan" }, target_side: "top" },
        { source: { node: "b", port: "plan" }, target: { node: "dst", port: "plan" }, target_side: "bottom" },
      ],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("__anchor:top");
    expect(edges[1].targetHandle).toBe("__anchor:bottom");
  });
});

describe("deriveLoopRegions — collection regions (#151)", () => {
  function node(id: string, type: NodeType, outputs: string[]): NodeDef {
    return {
      id,
      name: id,
      type,
      inputs: [],
      outputs: outputs.map((name) => ({ name, repeated: false, side: "right" as const })),
      interactive: false,
      view: { x: 200, y: 200 },
    };
  }

  function pipelineWith(nodes: NodeDef[], loops: LoopRegion[]): PipelineDef {
    return { name: "p", variables: {}, nodes, edges: [], loops };
  }

  it("renders a single-member collection as a `⇉ N items` badge, not a box", () => {
    // A single-member collection region (the common case — one Fixer per issue)
    // renders as a compact badge on the member card with the fan-out glyph `⇉`,
    // NOT a box and NOT the `↻` loop glyph.
    const p = pipelineWith(
      [node("fixer", "code-mutating", ["fix"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    );
    const regions = deriveLoopRegions(p, null);
    expect(regions).toHaveLength(1);
    const r = regions[0];
    expect(r.kind).toBe("collection");
    expect(r.box).toBeNull();
    expect(r.badgeMemberId).toBe("fixer");
    // Idle (no run): the badge shows the collection driver (`over <field>`),
    // never a `↻ i/max` loop counter.
    expect(r.counterText).toBe("over issues");
    expect(r.counterText).not.toContain("/");
    // A collection never exhausts (the lap count is the collection size).
    expect(r.exhausted).toBe(false);
  });

  it("renders a multi-member collection as a box", () => {
    const p = pipelineWith(
      [node("fix-a", "code-mutating", ["a"]), node("fix-b", "code-mutating", ["b"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fix-a", "fix-b"] }],
    );
    const regions = deriveLoopRegions(p, null);
    expect(regions[0].kind).toBe("collection");
    expect(regions[0].box).not.toBeNull();
    expect(regions[0].badgeMemberId).toBeNull();
  });

  it("attaches a `⇉` collection badge to the single member's card", () => {
    // The single-member collection's member card carries a `collectionBadge`
    // (the `⇉ ...` text) so the canvas can render the compact badge on the card
    // rather than a box.
    const p = pipelineWith(
      [node("fixer", "code-mutating", ["fix"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    );
    const cards = deriveEditNodes(p, null);
    const fixer = cards.find((c) => c.id === "fixer")!;
    expect(fixer.data.collectionBadge).toContain("⇉");
    expect(fixer.data.collectionBadge).toContain("over issues");
  });

  it("does not attach a collection badge to a node that is no member", () => {
    const p = pipelineWith(
      [node("fixer", "code-mutating", ["fix"]), node("other", "doc-only", ["x"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    );
    const cards = deriveEditNodes(p, null);
    const other = cards.find((c) => c.id === "other")!;
    expect(other.data.collectionBadge).toBeUndefined();
  });
});
