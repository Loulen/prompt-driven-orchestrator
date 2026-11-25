/**
 * A Run's canvas whose Start or End marker carries a `marker/item` row (ADR-0081,
 * amendment CORE-322): the decorated marker is a card's width and a fixed
 * height, taller than the 24 px pill every position was placed for, so the
 * rest of the pipeline goes down by as much (`lib/growLayout.ts`). Pure: it
 * returns a shifted COPY of the pipeline for the canvas to draw, never written
 * back. A Run canvas being read takes no gesture that writes a position.
 *
 * The compaction of the work cards (`compactCanvas.ts`) then runs on this
 * copy, the decorated markers at their drawn height.
 */
import { growBands, growShift, type GrowRect } from "../../lib/growLayout";
import { fallbackNodeSpot, fallbackNoteSpot } from "../../lib/nodePlacement";
import { COMPACT_CARD, estimateNoteHeight } from "./compactCanvas";
import type { EditorPipelineDef } from "./pipeline";

const isMarker = (type: string) => type === "start" || type === "end";

/** The drawn height of a marker: the pill, or the decorated card (`.cv-mk[data-decorated]`). */
export function markerHeight(decorated: boolean): number {
  return decorated ? COMPACT_CARD.decoratedMarker : COMPACT_CARD.marker;
}

export function markerGrownCanvas(pipeline: EditorPipelineDef, decorated: ReadonlySet<string>): EditorPipelineDef {
  if (decorated.size === 0) return pipeline;
  const rects: GrowRect[] = [];
  pipeline.nodes.forEach((n, i) => {
    const y = n.view?.y ?? fallbackNodeSpot(i).y;
    if (isMarker(n.type)) {
      rects.push({ id: n.id, y, h: COMPACT_CARD.marker, hGrown: markerHeight(decorated.has(n.id)) });
      return;
    }
    rects.push({ id: n.id, y, h: COMPACT_CARD.detailed, hGrown: COMPACT_CARD.detailed });
  });
  (pipeline.notes ?? []).forEach((note, i) => {
    const h = estimateNoteHeight(note.content);
    rects.push({ id: `note:${note.id}`, y: note.view?.y ?? fallbackNoteSpot(i).y, h, hGrown: h });
  });
  const bands = growBands(rects);
  if (bands.every((b) => b.gain === 0)) return pipeline;
  const down = <P extends { y: number }>(p: P): P => ({ ...p, y: p.y + growShift(bands, p.y) });

  const nodes = pipeline.nodes.map((n, i) => {
    const at = n.view ?? fallbackNodeSpot(i);
    if (growShift(bands, at.y) === 0 && n.view) return n;
    return { ...n, view: { ...(n.view ?? {}), ...down({ x: at.x, y: at.y }) } };
  });
  const edges = pipeline.edges.map((e) => ({
    ...e,
    waypoints: e.waypoints?.map(down),
    condition_label_pos: e.condition_label_pos ? down(e.condition_label_pos) : e.condition_label_pos,
    output_label_pos: e.output_label_pos
      ? Object.fromEntries(Object.entries(e.output_label_pos).map(([port, at]) => [port, down(at)]))
      : e.output_label_pos,
  }));
  const notes = pipeline.notes?.map((note, i) => {
    const at = note.view ?? fallbackNoteSpot(i);
    return { ...note, view: { ...(note.view ?? {}), ...down({ x: at.x, y: at.y }) } };
  });
  return { ...pipeline, nodes, edges, ...(notes ? { notes } : {}) };
}
