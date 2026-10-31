/**
 * A Run's canvas with « Compact nodes » on: every work card on one line (two
 * on an orchestrator with child runs), the rest of the pipeline risen by as
 * much (`lib/compactLayout.ts`). Pure: it returns a shifted COPY of the
 * pipeline for the canvas to draw, and never writes it back.
 *
 *  - A compact card keeps the axis of the detailed one: 168 px centred on the
 *    208 px card.
 *  - Its side ports fold onto the anchor of its first line; its top and bottom
 *    ports keep their abscissa, so vertical wires and their labels stay put.
 *  - Waypoints and pinned labels follow the shift of their own ordinate; a
 *    waypoint level with a folded side port follows the port, so the leg into
 *    it stays horizontal.
 *  - Start, End and a note do not shrink. A note follows the shift at its
 *    bottom unless it would then cover a card; it then holds its band.
 */
import type { EdgeAnchor } from "../../shared/generated/EdgeAnchor";
import type { EdgeWaypoint } from "../../shared/generated/EdgeWaypoint";
import type { PortSide } from "../../shared/generated/PortSide";
import { anchorPoint } from "../../lib/anchorSide";
import { boxesOverlap, compactLayout, type Box, type CompactPoint, type CompactRect } from "../../lib/compactLayout";
import { fallbackNodeSpot } from "../../lib/nodePlacement";
import type { EditorPipelineDef } from "./pipeline";

/**
 * The heights of the cards, in flow px, as `canvas.css` draws them: change
 * both together. A detailed card is its 24 px header, its two-row meta grid
 * (39 px) and its border; an orchestrator with child runs adds the « N runs »
 * row (18 px and its 4 px margin), on both sizes.
 */
export const COMPACT_CARD = {
  width: 208,
  detailed: 65,
  compactWidth: 168,
  compact: 30,
  runs: 22,
  marker: 24,
  noteWidth: 200,
} as const;

/** Where a folded side port sits on a compact card: the middle of its first line. */
export const COMPACT_PORT_Y = COMPACT_CARD.compact / 2;

const INSET = (COMPACT_CARD.width - COMPACT_CARD.compactWidth) / 2;
/** The rounding `useEdgeRoute` applies to an endpoint, so a leg is recognised as such. */
const LEVEL = 0.5;

export interface CompactCanvasInput {
  /** The orchestrators that have child runs: their cards carry « N runs » on both sizes. */
  withRuns?: ReadonlySet<string>;
}

export interface CompactCanvas {
  /** The pipeline as the compact canvas draws it. */
  pipeline: EditorPipelineDef;
  /** The compact box of every work card (top-left, size), keyed by node id. */
  cards: Record<string, Box>;
}

/**
 * The height of a note card (`.cv-note`: 164 px of 12 px text, 1.4 line
 * height, 14 px of padding and border), on the high side: a note is never
 * taller than its estimate, so one that clears the cards by its estimate
 * clears them for real.
 */
export function estimateNoteHeight(content: string): number {
  const lines = content.split("\n").reduce((n, line) => n + Math.max(1, Math.ceil((line.length * 7.5) / 164)), 0);
  return Math.ceil(lines * 17 + 16);
}

const isMarker = (type: string) => type === "start" || type === "end";

export function compactCanvas(pipeline: EditorPipelineDef, input: CompactCanvasInput = {}): CompactCanvas {
  const withRuns = input.withRuns ?? new Set<string>();
  const nodeRects: CompactRect[] = [];
  const detailed = new Map<string, { x: number; y: number; width: number; height: number }>();
  pipeline.nodes.forEach((n, i) => {
    const fallback = fallbackNodeSpot(i);
    const x = n.view?.x ?? fallback.x;
    const y = n.view?.y ?? fallback.y;
    if (isMarker(n.type)) {
      const w = Math.min(COMPACT_CARD.width, 40 + (n.name ?? n.id).length * 7);
      nodeRects.push({ id: n.id, x, y, w, h: COMPACT_CARD.marker, hCompact: COMPACT_CARD.marker });
      return;
    }
    const runs = withRuns.has(n.id) ? COMPACT_CARD.runs : 0;
    const h = COMPACT_CARD.detailed + runs;
    detailed.set(n.id, { x, y, width: COMPACT_CARD.width, height: h });
    nodeRects.push({ id: n.id, x, y, w: COMPACT_CARD.width, h, hCompact: COMPACT_CARD.compact + runs });
  });

  const notes = (pipeline.notes ?? []).map((note, i) => ({
    id: note.id,
    x: note.view?.x ?? 240,
    y: note.view?.y ?? 80 + i * 120,
    h: estimateNoteHeight(note.content),
  }));

  // The points that follow the canvas, keyed so they can be read back.
  const points: CompactPoint[] = [];
  pipeline.edges.forEach((e, i) => {
    e.waypoints?.forEach((w, j) => points.push({ id: `w:${i}:${j}`, ...w }));
    if (e.condition_label_pos) points.push({ id: `c:${i}`, ...e.condition_label_pos });
    for (const [port, at] of Object.entries(e.output_label_pos ?? {})) points.push({ id: `o:${i}:${port}`, ...at });
  });

  // A note rides as its bottom-left corner. One that would then land on a card
  // holds its band instead, and the layout is drawn again.
  const holding = new Set<string>();
  let layout = compactLayout(nodeRects, points);
  for (;;) {
    const rects = [...nodeRects, ...notes.filter((n) => holding.has(n.id)).map((n) => ({ id: `n:${n.id}`, x: n.x, y: n.y, w: COMPACT_CARD.noteWidth, h: n.h, hCompact: n.h }))];
    const riders = notes.filter((n) => !holding.has(n.id)).map((n) => ({ id: `n:${n.id}`, x: n.x, y: n.y + n.h }));
    layout = compactLayout(rects, [...points, ...riders]);
    const boxes = nodeRects.map((r) => cardBox(layout.rects[r.id], isMarker(typeOf(pipeline, r.id))));
    const blocked = notes.filter((n) => {
      if (holding.has(n.id)) return false;
      const at = layout.points[`n:${n.id}`];
      const box = { x: n.x, y: at.y - n.h, w: COMPACT_CARD.noteWidth, h: n.h };
      return boxes.some((b) => boxesOverlap(box, b));
    });
    if (blocked.length === 0) break;
    for (const n of blocked) holding.add(n.id);
  }

  const cards: Record<string, Box> = {};
  const nodes = pipeline.nodes.map((n) => {
    const r = layout.rects[n.id];
    if (!isMarker(n.type)) cards[n.id] = cardBox(r, false);
    return { ...n, view: { ...(n.view ?? {}), x: r.x, y: r.y } };
  });

  const shifted = (id: string) => layout.points[id];
  const edges = pipeline.edges.map((e, i) => {
    const source = detailed.get(e.source.node);
    const target = detailed.get(e.target.node);
    const sourceSide: PortSide = e.source_anchor?.side ?? "right";
    const targetSide: PortSide = e.target_anchor?.side ?? e.target_side ?? "left";
    let waypoints: EdgeWaypoint[] | null | undefined = e.waypoints?.map((_, j) => ({ ...shifted(`w:${i}:${j}`) }));
    if (waypoints && e.waypoints) {
      waypoints = followFoldedPort(waypoints, e.waypoints, source, sourceSide, e.source_anchor, cards[e.source.node]);
      waypoints = followFoldedPort([...waypoints].reverse(), [...e.waypoints].reverse(), target, targetSide, e.target_anchor, cards[e.target.node]).reverse();
    }
    const outputLabels = e.output_label_pos
      ? Object.fromEntries(Object.keys(e.output_label_pos).map((port) => [port, { ...shifted(`o:${i}:${port}`) }]))
      : e.output_label_pos;
    return {
      ...e,
      source_anchor: source ? foldAnchor(e.source_anchor, sourceSide) : e.source_anchor,
      target_anchor: target ? foldAnchor(e.target_anchor, targetSide) : e.target_anchor,
      waypoints,
      condition_label_pos: e.condition_label_pos ? { ...shifted(`c:${i}`) } : e.condition_label_pos,
      output_label_pos: outputLabels,
    };
  });

  const compactNotes = pipeline.notes?.map((note) => {
    const n = notes.find((m) => m.id === note.id)!;
    const y = holding.has(n.id) ? layout.rects[`n:${n.id}`].y : shifted(`n:${n.id}`).y - n.h;
    return { ...note, view: { ...(note.view ?? {}), x: n.x, y } };
  });

  return { pipeline: { ...pipeline, nodes, edges, ...(compactNotes ? { notes: compactNotes } : {}) }, cards };
}

function typeOf(pipeline: EditorPipelineDef, id: string): string {
  return pipeline.nodes.find((n) => n.id === id)?.type ?? "";
}

/** The drawn box: a work card narrows around its axis, a marker keeps its size. */
function cardBox(r: Box, marker: boolean): Box {
  return marker ? r : { x: r.x + INSET, y: r.y, w: COMPACT_CARD.compactWidth, h: r.h };
}

/**
 * The anchor of a compacted card's port: a side port folds onto the first
 * line (written even where the edge had no anchor: the middle of a two-line
 * card is not on its first line), a top or bottom one keeps its abscissa on
 * the narrower card.
 */
function foldAnchor(anchor: EdgeAnchor | null | undefined, side: PortSide): EdgeAnchor | null | undefined {
  if (side === "left" || side === "right") return { side, offset: COMPACT_PORT_Y };
  if (anchor?.offset == null) return anchor;
  return { ...anchor, offset: anchor.offset - INSET };
}

/**
 * The waypoints that run level with a side port, from the port on, follow the
 * port onto the compact card's first line: the leg into it stays straight.
 */
function followFoldedPort(
  moved: EdgeWaypoint[],
  original: EdgeWaypoint[],
  rect: { x: number; y: number; width: number; height: number } | undefined,
  side: PortSide,
  anchor: EdgeAnchor | null | undefined,
  card: Box | undefined,
): EdgeWaypoint[] {
  if (!rect || !card || (side !== "left" && side !== "right")) return moved;
  const portY = Math.round(anchorPoint(rect, anchor, side).y);
  const out = [...moved];
  for (let j = 0; j < original.length && Math.abs(original[j].y - portY) <= LEVEL; j++) {
    out[j] = { x: out[j].x, y: card.y + COMPACT_PORT_Y };
  }
  return out;
}
