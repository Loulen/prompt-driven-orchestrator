import { describe, it, expect } from "vitest";
import { deriveEditEdges, deriveEditNodes, deriveLoopRegions, runReachedEnd } from "./editNodeDerivation";
import type {
  LoopRegion,
  NodeDef,
  NodeType,
  PipelineDef,
  PortSide,
  RunState,
  RunStatus,
} from "../types";

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
      [node("src", "agent", [], ["plan"]), node("dst", "agent", [], ["code"])],
      [{ source: { node: "src", port: "plan" }, target: { node: "dst", port: "plan" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("__anchor:left");
    // #844: the arrow leaves by a RIM strip, not by a per-output handle — there
    // is no output dot to bind to any more. Un-anchored ⇒ the legacy rightwards
    // departure.
    expect(edges[0].sourceHandle).toBe("rim-right");
  });

  it("binds the departure to the rim of the side the wire was drawn from (#844)", () => {
    const p = pipeline(
      [node("src", "agent", [], ["plan"]), node("dst", "agent", [], ["code"])],
      [
        {
          source: { node: "src", port: "plan" },
          target: { node: "dst", port: "plan" },
          source_anchor: { side: "bottom", offset: 80 },
          target_anchor: { side: "top", offset: 36 },
          target_side: "top",
        },
      ],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].sourceHandle).toBe("rim-bottom");
    // The anchors ride on the edge data: the component draws from them, not
    // from xyflow's handle centres.
    expect(edges[0].data?.sourceAnchor).toEqual({ side: "bottom", offset: 80 });
    expect(edges[0].data?.targetAnchor).toEqual({ side: "top", offset: 36 });
  });

  it("keeps the declared port for the End node (it retains a `result` input handle)", () => {
    const p = pipeline(
      [node("src", "agent", [], ["plan"]), node("end", "end", ["result"], [])],
      [{ source: { node: "src", port: "plan" }, target: { node: "end", port: "result" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("result");
  });

  it("routes to the side the declared handle is actually ON (#844, FP finding 2)", () => {
    // A declared-port target has no say in `target_side` — nothing ever persists
    // one for it — so the field reads back as the `left` default while the handle
    // sits where the port declares it. Handing the edge that `left` laid the
    // perpendicular landing leg across a border the wire never touches, and the
    // approach to that phantom border was persisted as waypoints inside the card.
    const end = node("end", "end", ["result"], []);
    end.inputs[0].side = "top";
    const p = pipeline(
      [node("src", "agent", [], ["plan"]), end],
      [{ source: { node: "src", port: "plan" }, target: { node: "end", port: "result" } }],
    );
    expect(deriveEditEdges(p)[0].data?.targetSide).toBe("top");
  });

  it("still routes an emergent target to its persisted side", () => {
    const p = pipeline(
      [node("src", "agent", [], ["plan"]), node("dst", "agent", [], [])],
      [
        {
          source: { node: "src", port: "plan" },
          target: { node: "dst", port: "plan" },
          target_side: "bottom",
        },
      ],
    );
    expect(deriveEditEdges(p)[0].data?.targetSide).toBe("bottom");
  });

  it("keeps the declared port for structural nodes (merge)", () => {
    const p = pipeline(
      [node("src", "agent", [], ["plan"]), node("m", "merge", ["branches"], ["merged"])],
      [{ source: { node: "src", port: "plan" }, target: { node: "m", port: "branches" } }],
    );
    const edges = deriveEditEdges(p);
    expect(edges[0].targetHandle).toBe("branches");
  });

  it("binds both same-named edges to a side body handle when they pool into one body input (default left)", () => {
    const p = pipeline(
      [
        node("a", "agent", [], ["plan"]),
        node("b", "agent", [], ["plan"]),
        node("dst", "agent", [], ["code"]),
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
        node("a", "agent", [], ["plan"]),
        node("b", "agent", [], ["plan"]),
        node("dst", "agent", [], ["code"]),
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

describe("deriveEditEdges — work node with a vestigial declared `in` still anchors by side (#175)", () => {
  // The #149 migration to 0-input emergent nodes was never carried through to
  // node creation or the on-disk pipeline YAMLs, so a real work node usually
  // still carries a single declared `in`. The old `inputs.length === 1` gate
  // mistook that for a fixed-side declared port and forced every incoming edge
  // to the left handle. Anchoring is keyed on node TYPE now, so a 1-input work
  // node anchors by drop on any side, exactly like a migrated 0-input one.
  function workNode(id: string, type: NodeType, outputs: string[]): NodeDef {
    return {
      id,
      name: id,
      type,
      // The vestigial declared `in` (side left) that real pipelines still carry.
      inputs: [{ name: "in", repeated: false, side: "left" as const }],
      outputs: outputs.map((name) => ({ name, repeated: false, side: "right" as const })),
      interactive: false,
    };
  }

  function pipelineWith(targetSide: PortSide | undefined): PipelineDef {
    return {
      name: "p",
      variables: {},
      nodes: [
        {
          id: "src",
          name: "src",
          type: "agent",
          inputs: [],
          outputs: [{ name: "plan", repeated: false, side: "right" as const }],
          interactive: false,
        },
        workNode("dst", "agent", ["code"]),
      ],
      edges: [
        {
          source: { node: "src", port: "plan" },
          target: { node: "dst", port: "in" },
          target_side: targetSide,
        },
      ],
    };
  }

  it.each(["left", "right", "top", "bottom"] as const)(
    "binds the incoming edge to the %s body anchor (not the declared `in` handle)",
    (side) => {
      const edges = deriveEditEdges(pipelineWith(side));
      expect(edges[0].targetHandle).toBe(`__anchor:${side}`);
      // The route is told to arrive from that side, not forced left->right.
      expect(edges[0].data?.targetSide).toBe(side);
    },
  );

  it("defaults to the left body anchor when no target_side is persisted (legacy)", () => {
    const edges = deriveEditEdges(pipelineWith(undefined));
    expect(edges[0].targetHandle).toBe("__anchor:left");
    expect(edges[0].data?.targetSide).toBe("left");
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
      [node("fixer", "agent", ["fix"])],
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
      [node("fix-a", "agent", ["a"]), node("fix-b", "agent", ["b"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fix-a", "fix-b"] }],
    );
    const regions = deriveLoopRegions(p, null);
    expect(regions[0].kind).toBe("collection");
    expect(regions[0].box).not.toBeNull();
    expect(regions[0].badgeMemberId).toBeNull();
  });

  it("attaches a `⇉` collection badge to the single member's card", () => {
    // The single-member collection's member card carries a `loopBadge` (the
    // `⇉ ...` text, kind `collection`) so the canvas can render the compact badge
    // on the card rather than a box.
    const p = pipelineWith(
      [node("fixer", "agent", ["fix"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    );
    const cards = deriveEditNodes(p, null);
    const fixer = cards.find((c) => c.id === "fixer")!;
    const badge = fixer.data.loopBadge as { text: string; kind: string } | undefined;
    expect(badge?.kind).toBe("collection");
    expect(badge?.text).toContain("⇉");
    expect(badge?.text).toContain("over issues");
  });

  it("attaches a `↻` badge to a single-member bounded loop's card (#173)", () => {
    // A bounded region reduced to one present member (e.g. a member node was
    // deleted, leaving a self-loop survivor) draws no box, so it must render a
    // compact `↻ <counter>` badge — otherwise the loop is invisible on the
    // canvas. The glyph is the loop glyph, not the collection `⇉`.
    const p = pipelineWith(
      [node("worker", "agent", ["out"])],
      [{ id: "spin", kind: "bounded", members: ["worker"], max_iter: 4 }],
    );
    const cards = deriveEditNodes(p, null);
    const worker = cards.find((c) => c.id === "worker")!;
    const badge = worker.data.loopBadge as { text: string; kind: string } | undefined;
    expect(badge?.kind).toBe("bounded");
    expect(badge?.text).toContain("↻");
    expect(badge?.text).toContain("max 4");
    expect(badge?.text).not.toContain("⇉");
  });

  /// A live run where `fixer` reached lap `lapsReached`, inside a collection
  /// region resolved to `totalItems` (omit for a run with no projected region).
  function liveRun(lapsReached: number, totalItems?: number): RunState {
    return {
      run_id: "run-1",
      pipeline_name: "p",
      status: "running",
      input: null,
      started_at: null,
      completed_at: null,
      nodes: {
        fixer: {
          node_id: "fixer",
          status: "completed",
          iter: lapsReached,
          started_at: null,
          completed_at: null,
          failure_reason: null,
          iterations: [],
        },
      },
      edges: [],
      node_defs: [],
      start_node: null,
      end_node: null,
      merge_resolver: null,
      ...(totalItems == null
        ? {}
        : {
            collection_states: {
              "per-issue": {
                region_id: "per-issue",
                total_items: totalItems,
                done: false,
              },
            },
          }),
    };
  }

  const collectionPipeline = () =>
    pipelineWith(
      [node("fixer", "agent", ["fix"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    );

  it("reads the live collection badge as laps/total, not max(iter) (#453)", () => {
    // The denominator is the RESOLVED collection size, from the region's own
    // projected state — the only value that says how much work the region owes.
    const regions = deriveLoopRegions(collectionPipeline(), liveRun(1, 2));
    expect(regions[0].counterText).toBe("1/2 items");
  });

  it("distinguishes a region wedged at lap 1 of 2 from a finished 1-item region", () => {
    // THE #453 readability defect. Both runs have `max(iter) === 1`, so both used
    // to render `⇉ 1 items` on an all-green canvas: nothing on screen contradicted
    // "it's finished" while one of the two was frozen for ever.
    const wedged = deriveLoopRegions(collectionPipeline(), liveRun(1, 2))[0];
    const finished = deriveLoopRegions(collectionPipeline(), liveRun(1, 1))[0];
    expect(wedged.counterText).not.toBe(finished.counterText);
    expect(finished.counterText).toBe("1/1 items");
  });

  it("falls back to the bare lap count when the region has no projected state", () => {
    // Between the run starting and the fan-out resolving its `over` list there is
    // no `collection_states` entry; the badge must degrade, never print `1/0`.
    expect(deriveLoopRegions(collectionPipeline(), liveRun(1))[0].counterText).toBe(
      "1 items",
    );
  });

  it("does not attach a loop badge to a node that is no member", () => {
    const p = pipelineWith(
      [node("fixer", "agent", ["fix"]), node("other", "agent", ["x"])],
      [{ id: "per-issue", kind: "collection", over: "issues", members: ["fixer"] }],
    );
    const cards = deriveEditNodes(p, null);
    const other = cards.find((c) => c.id === "other")!;
    expect(other.data.loopBadge).toBeUndefined();
  });
});

// #843 / ADR-0073: an edge can carry several outputs of its source node. It is
// still ONE arrow on the canvas — drawn from the primary carried port's handle —
// and its condition pill names the output each predicate reads as soon as there
// is more than one to choose from.
describe("deriveEditEdges — multi-output edges (#843)", () => {
  function node(id: string, outputs: string[]): NodeDef {
    return {
      id,
      name: id,
      type: "agent",
      inputs: [],
      outputs: outputs.map((name) => ({ name, repeated: false, side: "right" as const })),
      interactive: false,
    };
  }

  function pipeline(edges: PipelineDef["edges"]): PipelineDef {
    return {
      name: "p",
      variables: {},
      nodes: [node("design", ["out", "spec"]), node("orch", [])],
      edges,
    };
  }

  it("draws ONE arrow for a multi-port edge, from the primary port's handle", () => {
    const edges = deriveEditEdges(
      pipeline([
        { source: { node: "design", ports: ["out", "spec"] }, target: { node: "orch", port: "out" } },
      ]),
    );
    expect(edges).toHaveLength(1);
    // One arrow, drawn from one place: the rim of the departure side (#844).
    expect(edges[0].sourceHandle).toBe("rim-right");
  });

  it("renders the condition pill with the output prefix once two ports are carried", () => {
    const edges = deriveEditEdges(
      pipeline([
        {
          source: { node: "design", ports: ["out", "spec"] },
          target: { node: "orch", port: "out" },
          when: { "out.has_design_work": { eq: true } },
        },
      ]),
    );
    expect(edges[0].data!.label).toBe("out.has_design_work = true");
  });

  it("renders the property alone on a single-port edge", () => {
    const edges = deriveEditEdges(
      pipeline([
        {
          source: { node: "design", port: "out" },
          target: { node: "orch", port: "out" },
          when: { has_design_work: { eq: true } },
        },
      ]),
    );
    expect(edges[0].data!.label).toBe("has_design_work = true");
  });
});
