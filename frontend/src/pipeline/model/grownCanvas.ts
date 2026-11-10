/**
 * A pipeline's edit canvas with its cards at their drawn height: a card lists
 * one line per output, so one with several outputs is taller than the 65 px
 * every position was placed for, and the rest of the pipeline goes down by as
 * much (`lib/growLayout.ts`, « Compact nodes » the other way round). Pure: it
 * returns a shifted COPY of the pipeline for the canvas to draw, and never
 * writes it back; a gesture that writes a position takes it back through
 * {@link GrownCanvas.toStored}.
 *
 *  - A pipeline whose cards have one output each is drawn exactly as stored.
 *  - Cards, Start/End, notes, waypoints and pinned labels follow the shift of
 *    their own ordinate.
 *  - A grown card keeps its side ports where they were, the middle of the
 *    65 px card: an edge that names no place on the side is given that one.
 */
import type { EdgeAnchor } from "../../shared/generated/EdgeAnchor";
import type { PortSide } from "../../shared/generated/PortSide";
import { landsByDrop } from "../../lib/anchorSide";
import type { Box } from "../../lib/compactLayout";
import { growBands, growShift, growUnshift, type GrowRect } from "../../lib/growLayout";
import { fallbackNodeSpot, fallbackNoteSpot } from "../../lib/nodePlacement";
import { COMPACT_CARD, estimateNoteHeight } from "./compactCanvas";
import type { EditorPipelineDef } from "./pipeline";

/**
 * The height of a card on the edit canvas, in flow px, as `canvas.css` draws
 * it: its 24 px header, its border, the meta grid's padding (7 px), the
 * `agent | worktree` line and one 16 px line per output. Change both together.
 */
export const EDIT_CARD = { base: 49, line: 16 } as const;

/** The drawn height of a work card listing `outputs` outputs: 65 px with one. */
export function editCardHeight(outputs: number): number {
  return EDIT_CARD.base + EDIT_CARD.line * outputs;
}

/** Where a side port sits when its edge names no place on the side: the middle of a one-output card. */
export const SIDE_PORT_Y = COMPACT_CARD.detailed / 2;

export interface GrownCanvas {
  /** The pipeline as the canvas draws it. */
  pipeline: EditorPipelineDef;
  /** The drawn box of every work card (top-left, size), keyed by node id. */
  cards: Record<string, Box>;
  /**
   * Where a point drawn at `y` is stored. `moving`: the boxes being moved,
   * left out of the layout the point is read against, so a card dragged away
   * from under its own growth does not carry it along.
   */
  toStored: (y: number, moving?: ReadonlySet<string>) => number;
}

const isMarker = (type: string) => type === "start" || type === "end";

export function grownCanvas(pipeline: EditorPipelineDef): GrownCanvas {
  const rects: (GrowRect & { x: number; w: number })[] = [];
  pipeline.nodes.forEach((n, i) => {
    const fallback = fallbackNodeSpot(i);
    const x = n.view?.x ?? fallback.x;
    const y = n.view?.y ?? fallback.y;
    if (isMarker(n.type)) {
      rects.push({ id: n.id, x, y, w: 0, h: COMPACT_CARD.marker, hGrown: COMPACT_CARD.marker });
      return;
    }
    rects.push({ id: n.id, x, y, w: COMPACT_CARD.width, h: COMPACT_CARD.detailed, hGrown: editCardHeight(n.outputs.length) });
  });
  (pipeline.notes ?? []).forEach((note, i) => {
    const y = note.view?.y ?? fallbackNoteSpot(i).y;
    const h = estimateNoteHeight(note.content);
    rects.push({ id: `note:${note.id}`, x: 0, y, w: 0, h, hGrown: h });
  });

  const bands = growBands(rects);
  const cards: Record<string, Box> = {};
  // Every card at the height it was placed for: the pipeline is drawn as stored.
  if (bands.every((b) => b.gain === 0)) return { pipeline, cards, toStored: (y) => y };
  const down = (y: number) => y + growShift(bands, y);
  const grew = new Set(rects.filter((r) => r.hGrown > r.h && !r.id.startsWith("note:")).map((r) => r.id));

  for (const r of rects) {
    if (r.w > 0) cards[r.id] = { x: r.x, y: down(r.y), w: r.w, h: r.hGrown };
  }

  const nodes = pipeline.nodes.map((n, i) => {
    const fallback = fallbackNodeSpot(i);
    const y = n.view?.y ?? fallback.y;
    if (growShift(bands, y) === 0 && n.view) return n;
    return { ...n, view: { ...(n.view ?? {}), x: n.view?.x ?? fallback.x, y: down(y) } };
  });

  const byId = new Map(pipeline.nodes.map((n) => [n.id, n]));
  const edges = pipeline.edges.map((e) => {
    const sourceSide: PortSide = e.source_anchor?.side ?? "right";
    const targetSide: PortSide | null = landsByDrop(byId.get(e.target.node)?.type ?? "agent")
      ? (e.target_anchor?.side ?? e.target_side ?? "left")
      : null;
    const shiftPoint = <P extends { x: number; y: number }>(p: P): P => ({ ...p, y: down(p.y) });
    return {
      ...e,
      source_anchor: grew.has(e.source.node) ? pinSidePort(e.source_anchor, sourceSide) : e.source_anchor,
      target_anchor: grew.has(e.target.node) && targetSide ? pinSidePort(e.target_anchor, targetSide) : e.target_anchor,
      waypoints: e.waypoints?.map(shiftPoint),
      condition_label_pos: e.condition_label_pos ? shiftPoint(e.condition_label_pos) : e.condition_label_pos,
      output_label_pos: e.output_label_pos
        ? Object.fromEntries(Object.entries(e.output_label_pos).map(([port, at]) => [port, shiftPoint(at)]))
        : e.output_label_pos,
    };
  });

  const notes = pipeline.notes?.map((note, i) => {
    const fallback = fallbackNoteSpot(i);
    const y = note.view?.y ?? fallback.y;
    return { ...note, view: { ...(note.view ?? {}), x: note.view?.x ?? fallback.x, y: down(y) } };
  });

  const toStored = (y: number, moving?: ReadonlySet<string>) => {
    if (!moving || moving.size === 0) return growUnshift(bands, y);
    return growUnshift(growBands(rects.filter((r) => !moving.has(r.id) && !moving.has(r.id.replace(/^note:/, "")))), y);
  };

  return { pipeline: { ...pipeline, nodes, edges, ...(notes ? { notes } : {}) }, cards, toStored };
}

/**
 * A side port of a grown card that names no place on its side keeps the
 * middle of the 65 px card it was drawn on, rather than following the middle
 * of the taller card down. Top and bottom ports, and a placed one, are kept.
 */
function pinSidePort(anchor: EdgeAnchor | null | undefined, side: PortSide): EdgeAnchor | null | undefined {
  if (side !== "left" && side !== "right") return anchor;
  if (anchor?.offset != null) return anchor;
  return { side, offset: SIDE_PORT_Y };
}
