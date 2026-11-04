import { describe, expect, it } from "vitest";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { PortSide } from "../../shared/generated/PortSide";
import { anchorPoint } from "../../lib/anchorSide";
import { boxesOverlap, type Box } from "../../lib/compactLayout";
import { COMPACT_CARD, compactCanvas, estimateNoteHeight, type CardSize } from "./compactCanvas";
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

const COL = 296;
const ROW = 140;
const at = (col: number, row: number) => ({ x: 40 + col * COL, y: 100 + row * ROW });

/**
 * A Run of 30 work nodes: a four-way fork, four columns, a loop with its back
 * edge, a merge, conditions pinned between the rows, an orchestrator beside
 * plain cards and a note beside a row.
 */
function catalogue(): EditorPipelineDef {
  const nodes: EditorNodeDef[] = [node("start", at(1, 0).x + 80, 20, "start"), node("fetch", at(1, 0).x, at(1, 0).y)];
  for (let col = 0; col < 4; col++) for (let row = 1; row <= 6; row++) nodes.push(node(`n${col}${row}`, at(col, row).x, at(col, row).y));
  nodes.push(node("approve", at(1, 7).x, at(1, 7).y), node("publish", at(1, 8).x, at(1, 8).y), node("rollback", at(2, 8).x, at(2, 8).y));
  nodes.push(node("post", at(0, 8).x, at(0, 8).y), node("archive", at(3, 8).x, at(3, 8).y));
  nodes.push(node("end", at(1, 9).x + 80, at(1, 9).y, "end"));
  const edges: EdgeDef[] = [{ source: { node: "start", port: "out" }, target: { node: "fetch", port: "in" }, target_side: "top" }];
  // The fork: from the bottom of fetch, one leg per column, each landing on a top.
  for (let col = 0; col < 4; col++) {
    const from = at(1, 0);
    const to = at(col, 1);
    const sx = from.x + 60 + col * 30;
    edges.push({
      source: { node: "fetch", port: "out" },
      source_anchor: { side: "bottom", offset: 60 + col * 30 },
      target: { node: `n${col}1`, port: "in" },
      target_side: "top",
      mode: "manual",
      waypoints: [
        { x: sx, y: from.y + 100 },
        { x: to.x + 104, y: from.y + 100 },
      ],
      output_label_pos: { out: { x: sx, y: from.y + 80 } },
    });
  }
  // The columns: auto edges, bottom to top.
  for (let col = 0; col < 4; col++)
    for (let row = 1; row < 6; row++)
      edges.push({ source: { node: `n${col}${row}`, port: "out" }, source_anchor: { side: "bottom" }, target: { node: `n${col}${row + 1}`, port: "in" }, target_side: "top" });
  // The loop: n03 → n04 on a condition, then back up from the right of n04 into the right of n03.
  edges.push({ source: { node: "n04", port: "out" }, target: { node: "n03", port: "in" }, target_side: "right", mode: "manual",
    waypoints: [
      { x: at(0, 4).x + 240, y: at(0, 4).y + 33 },
      { x: at(0, 4).x + 240, y: at(0, 3).y + 33 },
    ],
    when: { drift: { gt: 2 } }, condition_label_pos: { x: at(0, 4).x + 240, y: at(0, 3).y + 100 } });
  // The merge into approve, from the side of the last row: right → left, three legs.
  edges.push({ source: { node: "n06", port: "out" }, target: { node: "approve", port: "in" }, target_side: "left", mode: "manual",
    waypoints: [
      { x: at(0, 6).x + 240, y: at(0, 6).y + 33 },
      { x: at(0, 6).x + 240, y: at(1, 7).y + 33 },
    ] });
  edges.push({ source: { node: "approve", port: "out" }, target: { node: "publish", port: "in" }, when: { decision: { eq: "ship" } },
    condition_label_pos: { x: at(1, 7).x + 104, y: at(1, 7).y + 102 }, source_anchor: { side: "bottom" }, target_side: "top" });
  edges.push({ source: { node: "approve", port: "out" }, target: { node: "rollback", port: "in" }, else: true, source_anchor: { side: "right" }, target_side: "top" });
  edges.push({ source: { node: "publish", port: "out" }, target: { node: "end", port: "result" }, target_side: "top", source_anchor: { side: "bottom" } });
  return {
    name: "catalogue",
    version: "1.0",
    variables: {},
    nodes,
    edges,
    notes: [{ id: "note", content: "Stores pull the new catalogue at 06:00.", view: { x: at(2, 7).x, y: at(2, 7).y } }],
  };
}

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object") {
    Object.freeze(value);
    for (const v of Object.values(value)) deepFreeze(v);
  }
  return value;
}

type Rect = { x: number; y: number; width: number; height: number };
const detailedRect = (p: EditorPipelineDef, id: string, withRuns = new Set<string>()): Rect => {
  const n = p.nodes.find((m) => m.id === id)!;
  if (n.type === "start" || n.type === "end") return { x: n.view!.x, y: n.view!.y, width: 60, height: COMPACT_CARD.marker };
  return { x: n.view!.x, y: n.view!.y, width: COMPACT_CARD.width, height: COMPACT_CARD.detailed + (withRuns.has(id) ? COMPACT_CARD.runs : 0) };
};
const compactRect = (c: ReturnType<typeof compactCanvas>, id: string): Rect => {
  const card = c.cards[id];
  if (card) return { x: card.x, y: card.y, width: card.w, height: card.h };
  const n = c.pipeline.nodes.find((m) => m.id === id)!;
  return { x: n.view!.x, y: n.view!.y, width: 60, height: COMPACT_CARD.marker };
};

/** The drawn polyline of a manual edge: its two ports and its waypoints. */
function polyline(e: EdgeDef, src: Rect, tgt: Rect): { x: number; y: number }[] {
  const round = (p: { x: number; y: number }) => ({ x: Math.round(p.x), y: Math.round(p.y) });
  const srcSide: PortSide = e.source_anchor?.side ?? "right";
  const tgtSide: PortSide = e.target_anchor?.side ?? e.target_side ?? "left";
  return [round(anchorPoint(src, e.source_anchor, srcSide)), ...(e.waypoints ?? []), round(anchorPoint(tgt, e.target_anchor, tgtSide))];
}

const directions = (pts: { x: number; y: number }[]) =>
  pts.slice(1).map((p, i) => (p.y === pts[i].y ? "h" : p.x === pts[i].x ? "v" : "oblique"));

describe("compactCanvas", () => {
  it("never touches the pipeline it is given: off again draws the input as it was", () => {
    const pipeline = deepFreeze(catalogue());
    const before = JSON.stringify(pipeline);
    const c = compactCanvas(pipeline, { withRuns: new Set(["n32"]) });
    expect(JSON.stringify(pipeline)).toBe(before);
    expect(c.pipeline).not.toBe(pipeline);
  });

  it("draws a work card on one line of 168 × 30 on the axis of the detailed card, two lines for an orchestrator with runs", () => {
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns: new Set(["n32"]) });
    const fetch = pipeline.nodes.find((n) => n.id === "fetch")!;
    expect(c.cards.fetch).toEqual({ x: fetch.view!.x + 20, y: fetch.view!.y, w: 168, h: 30 });
    expect(c.cards.n32.h).toBe(52);
    expect(c.cards.start).toBeUndefined();
  });

  it("lifts every row by the details of the rows above it, leaving no gap", () => {
    const c = compactCanvas(catalogue());
    // Row 1 sits one detailed-minus-compact gain higher than in the detailed layout, row 6 six gains.
    expect(c.cards.n01.y).toBe(at(0, 1).y - 35);
    expect(c.cards.n06.y).toBe(at(0, 6).y - 6 * 35);
    expect(c.cards.n36.y).toBe(c.cards.n06.y);
  });

  it("lets no two cards, markers or notes overlap", () => {
    const c = compactCanvas(catalogue(), { withRuns: new Set(["n32"]) });
    const boxes: Box[] = c.pipeline.nodes.map((n) => {
      const r = compactRect(c, n.id);
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    });
    for (const note of c.pipeline.notes ?? []) boxes.push({ x: note.view!.x, y: note.view!.y, w: 200, h: estimateNoteHeight(note.content) });
    const pairs: [number, number][] = [];
    for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) if (boxesOverlap(boxes[i], boxes[j])) pairs.push([i, j]);
    expect(pairs).toEqual([]);
  });

  it("adds no bend to a manual edge, and a horizontal or vertical segment keeps its direction, forks and loops included", () => {
    const withRuns = new Set(["n32"]);
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns });
    pipeline.edges.forEach((e, i) => {
      if (e.mode !== "manual") return;
      const before = polyline(e, detailedRect(pipeline, e.source.node, withRuns), detailedRect(pipeline, e.target.node, withRuns));
      const ce = c.pipeline.edges[i];
      const after = polyline(ce, compactRect(c, ce.source.node), compactRect(c, ce.target.node));
      expect(directions(after), `edge ${i}`).toEqual(directions(before));
    });
  });

  it("folds a side port onto the first line, where its edge arrives; a top or bottom port keeps its abscissa", () => {
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns: new Set(["n32"]) });
    const merge = c.pipeline.edges.find((e) => e.source.node === "n06" && e.target.node === "approve")!;
    expect(merge.source_anchor).toEqual({ side: "right", offset: 15 });
    expect(merge.target_anchor).toEqual({ side: "left", offset: 15 });
    expect(merge.waypoints![0].y).toBe(c.cards.n06.y + 15);
    expect(merge.waypoints![1].y).toBe(c.cards.approve.y + 15);
    const fork = pipeline.edges.findIndex((e) => e.source.node === "fetch" && e.target.node === "n31");
    const before = anchorPoint(detailedRect(pipeline, "fetch"), pipeline.edges[fork].source_anchor, "bottom");
    const after = anchorPoint(compactRect(c, "fetch"), c.pipeline.edges[fork].source_anchor, "bottom");
    expect(after.x).toBe(before.x);
    expect(after.y).toBe(c.cards.fetch.y + 30);
    // On a two-line orchestrator too: its side ports leave its first line, not its middle.
    const orch = compactCanvas(pipeline, { withRuns: new Set(["approve"]) });
    const into = orch.pipeline.edges.find((e) => e.target.node === "approve")!;
    expect(anchorPoint(compactRect(orch, "approve"), into.target_anchor, "left").y).toBe(orch.cards.approve.y + 15);
  });

  it("puts no pinned label on a card", () => {
    const c = compactCanvas(catalogue(), { withRuns: new Set(["n32"]) });
    const cards = Object.values(c.cards);
    const labels = c.pipeline.edges.flatMap((e) => [
      ...(e.condition_label_pos ? [e.condition_label_pos] : []),
      ...Object.values(e.output_label_pos ?? {}),
    ]);
    expect(labels.length).toBeGreaterThan(4);
    const covered = labels.filter((p) => cards.some((r) => p.x > r.x && p.x < r.x + r.w && p.y > r.y && p.y < r.y + r.h));
    expect(covered).toEqual([]);
  });

  it("keeps a label pinned on the leg of a folded side port on that leg", () => {
    const pipeline: EditorPipelineDef = {
      name: "p",
      version: "1.0",
      variables: {},
      nodes: [node("a", 0, 0), node("b", 400, 0)],
      edges: [
        {
          source: { node: "a", port: "out" },
          target: { node: "b", port: "in" },
          when: { ok: { eq: true } },
          condition_label_pos: { x: 300, y: 33 },
          output_label_pos: { out: { x: 230, y: 33 } },
        },
      ],
    };
    const e = compactCanvas(pipeline).pipeline.edges[0];
    expect(e.condition_label_pos).toEqual({ x: 300, y: 15 });
    expect(e.output_label_pos).toEqual({ out: { x: 230, y: 15 } });
  });

  it("folds the arrival of a target that keeps a declared handle onto its left side, whatever the file says", () => {
    const pipeline: EditorPipelineDef = {
      name: "p",
      version: "1.0",
      variables: {},
      nodes: [node("a", 0, 0), { ...node("sw", 400, 0), type: "switch" }],
      edges: [{ source: { node: "a", port: "out" }, target: { node: "sw", port: "in" }, target_side: "right" }],
    };
    expect(compactCanvas(pipeline).pipeline.edges[0].target_anchor).toEqual({ side: "left", offset: 15 });
  });

  it("counts a wide character twice in a note's height", () => {
    expect(estimateNoteHeight("界".repeat(20))).toBeGreaterThan(estimateNoteHeight("a".repeat(20)));
  });

  it("lets a note that would land on a card hold its band", () => {
    // A tall note beside two rows: riding its bottom would lift it onto the card above it.
    const pipeline: EditorPipelineDef = {
      name: "p",
      version: "1.0",
      variables: {},
      nodes: [node("a", 0, 0), node("b", 300, 100), node("c", 300, 200), node("d", 0, 500)],
      edges: [],
      notes: [{ id: "tall", content: "a line\n".repeat(17), view: { x: 0, y: 80 } }],
    };
    const h = estimateNoteHeight(pipeline.notes![0].content);
    expect(h).toBeGreaterThan(270);
    const c = compactCanvas(pipeline);
    const note = c.pipeline.notes![0].view!;
    const box = { x: note.x, y: note.y, w: 200, h };
    expect(Object.values(c.cards).some((r) => boxesOverlap(box, r))).toBe(false);
    expect(note.y).toBe(80 - 35);
  });

  it("is the identity on a pipeline with nothing to shrink", () => {
    const pipeline: EditorPipelineDef = {
      name: "p",
      version: "1.0",
      variables: {},
      nodes: [node("start", 0, 0, "start"), node("end", 0, 100, "end")],
      edges: [{ source: { node: "start", port: "out" }, target: { node: "end", port: "result" }, mode: "manual", waypoints: [{ x: 30, y: 50 }] }],
    };
    expect(compactCanvas(pipeline).pipeline).toEqual(pipeline);
  });
});

/** The sizes a Run card takes with its elements hidden: one cell row, header only, and with « Compact nodes ». */
const SIZES: [string, CardSize][] = [
  ["one row of cells", { width: 208, height: 49 }],
  ["its header only", { width: 208, height: 26 }],
  ["one line (Compact nodes)", { width: 168, height: 30 }],
];

describe.each(SIZES)("compactCanvas, cards at %s", (_, card) => {
  // Mixed heights: orchestrators with runs on several rows, beside plain cards.
  const withRuns = new Set(["n32", "n04", "n22"]);
  const gain = COMPACT_CARD.detailed - card.height;

  it("never touches the pipeline it is given", () => {
    const pipeline = deepFreeze(catalogue());
    const before = JSON.stringify(pipeline);
    compactCanvas(pipeline, { withRuns, card });
    expect(JSON.stringify(pipeline)).toBe(before);
  });

  it("draws each card at its own height and width on the axis of the detailed card; Start and End keep theirs", () => {
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns, card });
    const fetch = pipeline.nodes.find((n) => n.id === "fetch")!;
    expect(c.cards.fetch).toEqual({ x: fetch.view!.x + (208 - card.width) / 2, y: fetch.view!.y, w: card.width, h: card.height });
    expect(c.cards.n32.h).toBe(card.height + COMPACT_CARD.runs);
    expect(c.cards.start).toBeUndefined();
    const end = c.pipeline.nodes.find((n) => n.id === "end")!;
    expect(end.view!.y).toBeLessThan(at(1, 9).y);
  });

  it("lifts every row by the gain of the rows above it, a row with an orchestrator included", () => {
    const c = compactCanvas(catalogue(), { withRuns, card });
    expect(c.cards.n01.y).toBe(at(0, 1).y - gain);
    expect(c.cards.n06.y).toBe(at(0, 6).y - 6 * gain);
    expect(c.cards.n36.y).toBe(c.cards.n06.y);
    // Row 4 holds an orchestrator: the cards beside it rise with it.
    expect(c.cards.n14.y).toBe(c.cards.n04.y);
  });

  it("lets no two cards, markers or notes overlap", () => {
    const c = compactCanvas(catalogue(), { withRuns, card });
    const boxes: Box[] = c.pipeline.nodes.map((n) => {
      const r = compactRect(c, n.id);
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    });
    for (const note of c.pipeline.notes ?? []) boxes.push({ x: note.view!.x, y: note.view!.y, w: 200, h: estimateNoteHeight(note.content) });
    const pairs: [number, number][] = [];
    for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) if (boxesOverlap(boxes[i], boxes[j])) pairs.push([i, j]);
    expect(pairs).toEqual([]);
  });

  it("adds no bend to a manual edge, forks, loops and merges included", () => {
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns, card });
    pipeline.edges.forEach((e, i) => {
      if (e.mode !== "manual") return;
      const before = polyline(e, detailedRect(pipeline, e.source.node, withRuns), detailedRect(pipeline, e.target.node, withRuns));
      const ce = c.pipeline.edges[i];
      const after = polyline(ce, compactRect(c, ce.source.node), compactRect(c, ce.target.node));
      expect(directions(after), `edge ${i}`).toEqual(directions(before));
    });
  });

  it("folds a side port onto the middle of the card's first part, where its edge arrives; a bottom port keeps its abscissa", () => {
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns, card });
    const portY = Math.floor(card.height / 2);
    const merge = c.pipeline.edges.find((e) => e.source.node === "n06" && e.target.node === "approve")!;
    expect(merge.source_anchor).toEqual({ side: "right", offset: portY });
    expect(merge.target_anchor).toEqual({ side: "left", offset: portY });
    expect(merge.waypoints![1].y).toBe(c.cards.approve.y + portY);
    const fork = pipeline.edges.findIndex((e) => e.source.node === "fetch" && e.target.node === "n31");
    const before = anchorPoint(detailedRect(pipeline, "fetch"), pipeline.edges[fork].source_anchor, "bottom");
    const after = anchorPoint(compactRect(c, "fetch"), c.pipeline.edges[fork].source_anchor, "bottom");
    expect(after.x).toBe(before.x);
    expect(after.y).toBe(c.cards.fetch.y + card.height);
  });

  it("puts no pinned label on a card, and keeps every label", () => {
    const pipeline = catalogue();
    const c = compactCanvas(pipeline, { withRuns, card });
    const labelsOf = (p: EditorPipelineDef) =>
      p.edges.flatMap((e) => [...(e.condition_label_pos ? [e.condition_label_pos] : []), ...Object.values(e.output_label_pos ?? {})]);
    const labels = labelsOf(c.pipeline);
    expect(labels).toHaveLength(labelsOf(pipeline).length);
    const cards = Object.values(c.cards);
    expect(labels.filter((p) => cards.some((r) => p.x > r.x && p.x < r.x + r.w && p.y > r.y && p.y < r.y + r.h))).toEqual([]);
  });

  it("lets a note ride beside its row", () => {
    const c = compactCanvas(catalogue(), { withRuns, card });
    const note = c.pipeline.notes![0].view!;
    expect(note.y).toBeLessThan(at(2, 7).y);
    expect(note.x).toBe(at(2, 7).x);
  });
});
