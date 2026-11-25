import { useEffect, useMemo } from "react";
import type { Node } from "@xyflow/react";
import { useAppearance } from "../../hooks/useAppearance";
import { isVisible } from "../../lib/appearance";
import { compactCanvas } from "../model/compactCanvas";
import { markerGrownCanvas } from "../model/markerCanvas";
import { useDecoratedMarkers, type MarkerOfRun } from "../../shared/extensions/markerContext";
import { runCardSize } from "../model/runCardElements";
import { grownCanvas } from "../model/grownCanvas";
import { IDENTITY_SHIFT, type CanvasShift } from "./canvasShift";
import type { EditorPipelineDef } from "../model/pipeline";
import { isRunTabLocked, type OpenPipeline } from "./editState";

/**
 * A Run canvas that is not being edited draws its cards as the Apparence says
 * (`model/runCardElements.ts`): on one line with « Compact nodes », else at the
 * height of the elements it shows, from a shifted copy of the pipeline;
 * nothing is written. Its cards carry `runCanvas`, so they leave out the
 * elements the Apparence hides. A Run edited for itself, like the editor,
 * keeps the detailed cards: a drag there writes positions. `shown` is the
 * pipeline to draw, `placed` the derived cards at their drawn place.
 *
 * The other way round, a template's card lists one line per output: a card
 * with several is taller, and the rest of the pipeline goes down by as much
 * (`model/grownCanvas.ts`), again from a shifted copy. `shift` takes a drawn
 * position back to the stored one for the gestures that write one.
 */
export function useCanvasCompaction(
  tab: OpenPipeline | undefined,
  pipeline: EditorPipelineDef | undefined,
  cards: Node[],
  template = false,
  /** Kept on the current shift, for the gestures that run before this render's. */
  shiftRef?: React.RefObject<CanvasShift>,
) {
  const appearance = useAppearance();
  const runCanvas = isRunTabLocked(tab);
  const size = runCanvas ? runCardSize((id) => isVisible(appearance, id), appearance.compactNodes) : null;
  // As two numbers, so an Apparence change that keeps the size lays out nothing again.
  const width = size?.width ?? 0;
  const height = size?.height ?? 0;
  const compact = runCanvas && appearance.compactNodes;
  // The orchestrators with child runs carry « N runs » on both sizes: their
  // ids as one string, so a new array with the same content changes nothing.
  const withRuns = useMemo(
    () => JSON.stringify(cards.filter((c) => (c.data as { childCounts?: unknown }).childCounts != null).map((c) => c.id)),
    [cards],
  );
  const runId = tab?.runId;
  const markers = useMemo(() => (runCanvas && runId ? markersOf(runId, cards) : []), [runCanvas, runId, cards]);
  const decoratedIds = useDecoratedMarkers(markers);
  const decorated = useMemo(() => new Set(decoratedIds ? decoratedIds.split("\n") : []), [decoratedIds]);
  const base = useMemo(
    () => (runCanvas && pipeline ? markerGrownCanvas(pipeline, decorated) : pipeline),
    [runCanvas, pipeline, decorated],
  );
  const compaction = useMemo(
    () =>
      height > 0 && base
        ? compactCanvas(base, { withRuns: new Set(JSON.parse(withRuns) as string[]), card: { width, height }, decorated })
        : null,
    [width, height, base, withRuns, decorated],
  );
  const growth = useMemo(
    () => (!runCanvas && template && pipeline ? grownCanvas(pipeline) : null),
    [runCanvas, template, pipeline],
  );
  const grown = growth && growth.pipeline !== pipeline ? growth : null;
  const placed = useMemo(() => {
    if (grown) {
      const views = new Map(grown.pipeline.nodes.map((n) => [n.id, n.view]));
      return cards.map((card) => {
        const view = views.get(card.id);
        return view && view.y !== card.position.y ? { ...card, position: { x: card.position.x, y: view.y } } : card;
      });
    }
    if (!runCanvas) return cards;
    const views = new Map((compaction?.pipeline ?? base)?.nodes.map((n) => [n.id, n.view]));
    return cards.map((card) => {
      const box = compaction?.cards[card.id];
      if (box) return { ...card, position: { x: box.x, y: box.y }, data: { ...card.data, runCanvas: true, ...(compact ? { compact: true } : {}) } };
      const view = views.get(card.id);
      const marker = decorated.has(card.id) ? { decorated: true, runId } : {};
      return { ...card, ...(view ? { position: { x: view.x, y: view.y } } : {}), data: { ...card.data, runCanvas: true, ...marker } };
    });
  }, [compaction, grown, cards, runCanvas, compact, base, decorated, runId]);
  const shown = compaction?.pipeline ?? grown?.pipeline ?? base;
  // One stable identity when nothing is shifted: the edges reading it do not
  // re-render on every edit, and a write skips the conversion outright.
  const shift: CanvasShift = useMemo(
    () => (grown ? { shown: grown.pipeline, toStored: grown.toStored } : IDENTITY_SHIFT),
    [grown],
  );
  useEffect(() => {
    if (shiftRef) shiftRef.current = shift;
  }, [shiftRef, shift]);
  return { shown, cards: compaction?.cards ?? grown?.cards, placed, shift };
}

/** The Start and End markers among the derived cards, as `marker/item` sees them. */
function markersOf(run: string, cards: Node[]): MarkerOfRun[] {
  return cards.flatMap((card) => {
    const data = card.data as { nodeType?: string; label?: string; reached?: boolean };
    if (data.nodeType !== "start" && data.nodeType !== "end") return [];
    const type = data.nodeType;
    return [{ run, node: card.id, name: data.label ?? null, type, reached: type === "start" || data.reached === true }];
  });
}
