import { describe, expect, it } from "vitest";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import { editCardHeight, grownCanvas, SIDE_PORT_Y } from "./grownCanvas";
import type { EditorNodeDef, EditorPipelineDef } from "./pipeline";

const node = (id: string, x: number, y: number, outputs = 1, type: EditorNodeDef["type"] = "agent"): EditorNodeDef => ({
  id,
  name: id,
  type,
  inputs: [],
  outputs: Array.from({ length: outputs }, (_, i) => ({ name: i === 0 ? "out" : `out${i}` })),
  interactive: false,
  view: { x, y },
});

/** Grill → Design (three outputs) → Implement on one row; Review under Design, End under Implement. */
function pipeline(designOutputs = 3): EditorPipelineDef {
  const edges: EdgeDef[] = [
    { source: { node: "grill", port: "out" }, target: { node: "design", port: "in" } },
    {
      source: { node: "design", port: "out" },
      source_anchor: { side: "bottom", offset: 104 },
      target: { node: "review", port: "in" },
      target_side: "top",
    },
    {
      source: { node: "review", port: "out" },
      target: { node: "design", port: "in" },
      target_side: "right",
      mode: "manual",
      waypoints: [
        { x: 640, y: 332 },
        { x: 640, y: 132 },
      ],
      condition_label_pos: { x: 600, y: 250 },
    },
  ];
  return {
    name: "p",
    version: "1.0",
    variables: {},
    nodes: [
      node("start", 0, 120, 1, "start"),
      node("grill", 100, 100),
      node("design", 400, 100, designOutputs),
      node("implement", 700, 100, 2),
      node("review", 400, 300),
      node("end", 760, 300, 0, "end"),
    ],
    edges,
    notes: [{ id: "n1", content: "Review loops back", view: { x: 100, y: 300 } }],
  };
}

const viewY = (p: EditorPipelineDef, id: string) => p.nodes.find((n) => n.id === id)!.view!.y;

describe("grownCanvas", () => {
  it("is 65 px tall with one output and 16 px more per output", () => {
    expect(editCardHeight(1)).toBe(65);
    expect(editCardHeight(3)).toBe(97);
  });

  it("draws a pipeline whose cards have one output each exactly as stored", () => {
    const p = pipeline(1);
    p.nodes = p.nodes.map((n) => (n.id === "implement" ? { ...n, outputs: [{ name: "out" }] } : n));
    const grown = grownCanvas(p);
    expect(grown.pipeline).toBe(p);
    expect(grown.toStored(500)).toBe(500);
  });

  it("moves everything under the tallest card of a row down by its growth, nothing on the row", () => {
    const grown = grownCanvas(pipeline()).pipeline;
    // Design grows by 32 (three outputs), Implement by 16: the row gives 32.
    expect(viewY(grown, "grill")).toBe(100);
    expect(viewY(grown, "design")).toBe(100);
    expect(viewY(grown, "start")).toBe(120);
    expect(viewY(grown, "review")).toBe(332);
    expect(viewY(grown, "end")).toBe(332);
    expect(grown.notes![0].view!.y).toBe(332);
    expect(grown.edges[2].waypoints).toEqual([
      { x: 640, y: 364 },
      { x: 640, y: 132 },
    ]);
    expect(grown.edges[2].condition_label_pos).toEqual({ x: 600, y: 282 });
  });

  it("keeps the side ports of a grown card at the height they had", () => {
    const grown = grownCanvas(pipeline()).pipeline;
    // Grill → Design lands on Design's left at the middle of a 65 px card, not of 97 px.
    expect(grown.edges[0].target_anchor).toEqual({ side: "left", offset: SIDE_PORT_Y });
    // Review → Design lands on its right, at the same height: its waypoint at 132 stays level.
    expect(grown.edges[2].target_anchor).toEqual({ side: "right", offset: SIDE_PORT_Y });
    // A bottom port keeps its own place.
    expect(grown.edges[1].source_anchor).toEqual({ side: "bottom", offset: 104 });
    // Grill did not grow: its edge is left as it was.
    expect(grown.edges[0].source_anchor).toBeUndefined();
  });

  it("gives each work card its drawn box", () => {
    const { cards } = grownCanvas(pipeline());
    expect(cards.design).toEqual({ x: 400, y: 100, w: 208, h: 97 });
    expect(cards.review).toEqual({ x: 400, y: 332, w: 208, h: 65 });
  });

  it("stores a drawn position back where it is drawn from", () => {
    const grown = grownCanvas(pipeline());
    expect(grown.toStored(332)).toBe(300);
    expect(grown.toStored(100)).toBe(100);
  });

  it("reads a dragged card against the layout without it", () => {
    const grown = grownCanvas(pipeline());
    // Design, the tallest of its row, dragged below the review: Implement still gives 16.
    expect(grown.toStored(500, new Set(["design"]))).toBe(484);
  });
});
