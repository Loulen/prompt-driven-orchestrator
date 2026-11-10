/**
 * A Run's canvas whose work cards are shorter than the detailed card every
 * position was placed for, the rest of the pipeline risen by as much
 * (`lib/compactLayout.ts`): with « Compact nodes » on, every card on one line
 * (two on an orchestrator with child runs); else each card at the height of
 * the elements the Apparence shows (`runCardElements.ts`). Pure: it returns a
 * shifted COPY of the pipeline for the canvas to draw, and never writes it back.
 *
 *  - A narrower card keeps the axis of the detailed one: 168 px centred on the
 *    208 px card.
 *  - Its side ports fold onto the middle of its first part (the card without
 *    an orchestrator's runs row); its top and bottom ports keep their abscissa,
 *    so vertical wires and their labels stay put.
 *  - Waypoints and pinned labels follow the shift of their own ordinate; a
 *    waypoint level with a folded side port follows the port, so the leg into
 *    it stays horizontal.
 *  - Start, End and a note do not shrink. A note follows the shift at its
 *    bottom unless it would then cover a card; it then holds its band.
 */
import type { EdgeAnchor } from "../../shared/generated/EdgeAnchor";
import type { EdgeWaypoint } from "../../shared/generated/EdgeWaypoint";
import type { PortSide } from "../../shared/generated/PortSide";
import { anchorPoint, landsByDrop } from "../../lib/anchorSide";
import { boxesOverlap, compactLayout, type Box, type CompactPoint, type CompactRect } from "../../lib/compactLayout";
import { fallbackNodeSpot, fallbackNoteSpot } from "../../lib/nodePlacement";
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

/** The drawn size of a shorter work card: its width, its height without an orchestrator's runs row. */
export interface CardSize {
  width: number;
  height: number;
}

/** A card on one line (« Compact nodes »). */
export const COMPACT_SIZE: CardSize = { width: COMPACT_CARD.compactWidth, height: COMPACT_CARD.compact };

/** Where a side port folds on a card of `size`: the middle of its first part, on a whole px. */
export function foldedPortY(size: CardSize): number {
  return Math.floor(size.height / 2);
}
/** The rounding `useEdgeRoute` applies to an endpoint, so a leg is recognised as such. */
const LEVEL = 0.5;

export interface CompactCanvasInput {
  /** The orchestrators that have child runs: their cards carry « N runs » on every size. */
  withRuns?: ReadonlySet<string>;
  /** The size every work card is drawn at: one line by default. */
  card?: CardSize;
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
 * clears them for real. A wide character (CJK, emoji) counts twice.
 */
export function estimateNoteHeight(content: string): number {
  const width = (line: string) => [...line].reduce((w, ch) => w + ((ch.codePointAt(0) ?? 0) >= 0x2e80 ? 15 : 7.5), 0);
  const lines = content.split("\n").reduce((n, line) => n + Math.max(1, Math.ceil(width(line) / 164)), 0);
  return Math.ceil(lines * 17 + 16);
}

const isMarker = (type: string) => type === "start" || type === "end";

export function compactCanvas(pipeline: EditorPipelineDef, input: CompactCanvasInput = {}): CompactCanvas {
  const withRuns = input.withRuns ?? new Set<string>();
  const size = input.card ?? COMPACT_SIZE;
  const portY = foldedPortY(size);
  const byId = new Map(pipeline.nodes.map((n) => [n.id, n]));
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
    nodeRects.push({ id: n.id, x, y, w: COMPACT_CARD.width, h, hCompact: size.height + runs });
  });

  const notes = (pipeline.notes ?? []).map((note, i) => ({
    id: note.id,
    ...(note.view ? { x: note.view.x, y: note.view.y } : fallbackNoteSpot(i)),
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
    const boxes = nodeRects.map((r) => cardBox(layout.rects[r.id], isMarker(byId.get(r.id)!.type), size));
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
    if (!isMarker(n.type)) cards[n.id] = cardBox(r, false, size);
    return { ...n, view: { ...(n.view ?? {}), x: r.x, y: r.y } };
  });

  const shifted = (id: string) => layout.points[id];
  const edges = pipeline.edges.map((e, i) => {
    const source = detailed.get(e.source.node);
    const target = detailed.get(e.target.node);
    const sourceSide: PortSide = e.source_anchor?.side ?? "right";
    // The side the arrow lands on: a target that keeps a declared handle lands on the left, whatever the file says.
    const targetSide: PortSide = landsByDrop(byId.get(e.target.node)?.type ?? "agent")
      ? (e.target_anchor?.side ?? e.target_side ?? "left")
      : "left";
    const from = foldedPort(source, sourceSide, e.source_anchor, cards[e.source.node], portY);
    const to = foldedPort(target, targetSide, e.target_anchor, cards[e.target.node], portY);
    let waypoints: EdgeWaypoint[] | null | undefined = e.waypoints?.map((_, j) => ({ ...shifted(`w:${i}:${j}`) }));
    if (waypoints && e.waypoints) {
      waypoints = followFoldedPort(waypoints, e.waypoints, from);
      waypoints = followFoldedPort([...waypoints].reverse(), [...e.waypoints].reverse(), to).reverse();
    }
    // A pinned label rides the shift too, unless it sits level with a folded port: it then stays on the leg.
    const label = (id: string, at: EdgeWaypoint) => ({ x: at.x, y: levelWith(at, from) ?? levelWith(at, to) ?? shifted(id).y });
    const outputLabels = e.output_label_pos
      ? Object.fromEntries(Object.entries(e.output_label_pos).map(([port, at]) => [port, label(`o:${i}:${port}`, at)]))
      : e.output_label_pos;
    return {
      ...e,
      source_anchor: source ? foldAnchor(e.source_anchor, sourceSide, size) : e.source_anchor,
      target_anchor: target ? foldAnchor(e.target_anchor, targetSide, size) : e.target_anchor,
      waypoints,
      condition_label_pos: e.condition_label_pos ? label(`c:${i}`, e.condition_label_pos) : e.condition_label_pos,
      output_label_pos: outputLabels,
    };
  });

  const compactNotes = pipeline.notes?.map((note, i) => {
    const n = notes[i];
    const y = holding.has(n.id) ? layout.rects[`n:${n.id}`].y : shifted(`n:${n.id}`).y - n.h;
    return { ...note, view: { ...(note.view ?? {}), x: n.x, y } };
  });

  return { pipeline: { ...pipeline, nodes, edges, ...(compactNotes ? { notes: compactNotes } : {}) }, cards };
}

/** How far a card of `size` sits inside the detailed one, on each side. */
const inset = (size: CardSize) => (COMPACT_CARD.width - size.width) / 2;

/** The drawn box: a work card narrows around its axis, a marker keeps its size. */
function cardBox(r: Box, marker: boolean, size: CardSize): Box {
  return marker ? r : { x: r.x + inset(size), y: r.y, w: size.width, h: r.h };
}

/**
 * The anchor of a shorter card's port: a side port folds onto the middle of
 * its first part (written even where the edge had no anchor: the middle of a
 * card with a runs row is not there), a top or bottom one keeps its abscissa
 * on a narrower card. Within 20 px of a corner of the detailed card there is
 * no such abscissa on the compact one: the port is clamped, and its wire jogs.
 */
function foldAnchor(anchor: EdgeAnchor | null | undefined, side: PortSide, size: CardSize): EdgeAnchor | null | undefined {
  if (side === "left" || side === "right") return { side, offset: foldedPortY(size) };
  if (anchor?.offset == null || inset(size) === 0) return anchor;
  return { ...anchor, offset: anchor.offset - inset(size) };
}

/** A side port of a compacted card: its ordinate on the detailed card, and on the compact one. */
interface FoldedPort {
  before: number;
  after: number;
}

function foldedPort(
  rect: { x: number; y: number; width: number; height: number } | undefined,
  side: PortSide,
  anchor: EdgeAnchor | null | undefined,
  card: Box | undefined,
  portY: number,
): FoldedPort | null {
  if (!rect || !card || (side !== "left" && side !== "right")) return null;
  return { before: Math.round(anchorPoint(rect, anchor, side).y), after: card.y + portY };
}

/** The folded port's new ordinate when `at` ran level with it, else null. */
function levelWith(at: EdgeWaypoint, port: FoldedPort | null): number | null {
  return port && Math.abs(at.y - port.before) <= LEVEL ? port.after : null;
}

/**
 * The waypoints that run level with a side port, from the port on, follow the
 * port onto the compact card's first line: the leg into it stays straight.
 */
function followFoldedPort(moved: EdgeWaypoint[], original: EdgeWaypoint[], port: FoldedPort | null): EdgeWaypoint[] {
  if (!port) return moved;
  const out = [...moved];
  for (let j = 0; j < original.length && levelWith(original[j], port) != null; j++) out[j] = { x: out[j].x, y: port.after };
  return out;
}
