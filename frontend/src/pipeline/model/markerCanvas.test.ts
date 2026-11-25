import { describe, expect, it } from "vitest";
import { boxesOverlap, type Box } from "../../lib/compactLayout";
import { COMPACT_CARD, COMPACT_SIZE, compactCanvas, estimateNoteHeight } from "./compactCanvas";
import { markerGrownCanvas, markerHeight } from "./markerCanvas";
import type { EditorNodeDef, EditorPipelineDef } from "./pipeline";

const node = (id: string, x: number, y: number, type: EditorNodeDef["type"] = "agent"): EditorNodeDef => ({
  id,
  name: id,
  type,
  inputs: [],
  outputs: type === "end" ? [] : [{ name: "out" }],
  interactive: false,
  view: { x, y },
});

/** Start, two work cards one under the other, End, a waypoint and a note between them. */
function straight(): EditorPipelineDef {
  return {
    name: "straight",
    version: "1.0",
    variables: {},
    nodes: [node("start", 100, 0, "start"), node("work", 100, 60, "agent"), node("review", 100, 160), node("end", 100, 260, "end")],
    edges: [
      { source: { node: "start", port: "out" }, target: { node: "work", port: "in" }, target_side: "top" },
      {
        source: { node: "review", port: "out" },
        target: { node: "end", port: "result" },
        target_side: "top",
        mode: "manual",
        waypoints: [{ x: 300, y: 240 }],
        condition_label_pos: { x: 300, y: 245 },
      },
    ],
    notes: [{ id: "note", content: "A note beside the work.", view: { x: 400, y: 100 } }],
  };
}

const yOf = (p: EditorPipelineDef, id: string) => p.nodes.find((n) => n.id === id)!.view!.y;

/** Every drawn box: work cards at their compact size, markers at theirs, notes. */
function boxes(c: ReturnType<typeof compactCanvas>, decorated: ReadonlySet<string>): Box[] {
  const out: Box[] = c.pipeline.nodes.map((n) => {
    if (c.cards[n.id]) return c.cards[n.id];
    const w = decorated.has(n.id) ? COMPACT_CARD.width : 60;
    return { x: n.view!.x, y: n.view!.y, w, h: markerHeight(decorated.has(n.id)) };
  });
  for (const note of c.pipeline.notes ?? []) out.push({ x: note.view!.x, y: note.view!.y, w: 200, h: estimateNoteHeight(note.content) });
  return out;
}

describe("a decorated marker on a Run canvas", () => {
  it("is 47 px high, as the decorated `.cv-mk` draws it: border, 24 px header, rule, 20 px row, border", () => {
    expect(COMPACT_CARD.decoratedMarker).toBe(1 + 24 + 1 + 20 + 1);
    expect(markerHeight(true)).toBe(47);
    expect(markerHeight(false)).toBe(COMPACT_CARD.marker);
  });

  it("draws the pipeline as stored when no marker is decorated", () => {
    const pipeline = straight();
    expect(markerGrownCanvas(pipeline, new Set())).toBe(pipeline);
  });

  it("takes the rest of the pipeline down by its extra height, waypoints, labels and notes included", () => {
    const pipeline = straight();
    const before = JSON.stringify(pipeline);
    const grown = markerGrownCanvas(pipeline, new Set(["start"]));
    expect(JSON.stringify(pipeline)).toBe(before);
    const gain = COMPACT_CARD.decoratedMarker - COMPACT_CARD.marker;
    expect(yOf(grown, "start")).toBe(0);
    expect(yOf(grown, "work")).toBe(60 + gain);
    expect(yOf(grown, "review")).toBe(160 + gain);
    expect(yOf(grown, "end")).toBe(260 + gain);
    expect(grown.edges[1].waypoints).toEqual([{ x: 300, y: 240 + gain }]);
    expect(grown.edges[1].condition_label_pos).toEqual({ x: 300, y: 245 + gain });
    expect(grown.notes![0].view!.y).toBe(100 + gain);
  });

  it("moves nothing for a decorated End at the bottom", () => {
    const pipeline = straight();
    const grown = markerGrownCanvas(pipeline, new Set(["end"]));
    expect(grown.nodes.map((n) => n.view!.y)).toEqual([0, 60, 160, 260]);
  });

  it("keeps its card width and height through the compaction, which still lifts the work cards", () => {
    const decorated = new Set(["start", "end"]);
    const c = compactCanvas(markerGrownCanvas(straight(), decorated), { card: COMPACT_SIZE, decorated });
    const pairs: [number, number][] = [];
    const all = boxes(c, decorated);
    for (let i = 0; i < all.length; i++) for (let j = i + 1; j < all.length; j++) if (boxesOverlap(all[i], all[j])) pairs.push([i, j]);
    expect(pairs).toEqual([]);
    // Compact nodes keeps the row: the markers are not shrunk, the work cards are.
    expect(c.cards.start).toBeUndefined();
    expect(c.cards.work.h).toBe(COMPACT_CARD.compact);
    // Start's bottom, then the work card under it, then End under the review card.
    expect(c.cards.work.y).toBeGreaterThanOrEqual(yOf(c.pipeline, "start") + 47);
    expect(yOf(c.pipeline, "end")).toBeGreaterThanOrEqual(c.cards.review.y + c.cards.review.h);
    // The work cards rise against the grown layout: less than the stored distance between them.
    expect(c.cards.review.y - c.cards.work.y).toBeLessThan(100);
  });
});
