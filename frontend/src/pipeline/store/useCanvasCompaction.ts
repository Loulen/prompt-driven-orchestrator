import { useEffect, useMemo } from "react";
import type { Node } from "@xyflow/react";
import { useAppearance } from "../../hooks/useAppearance";
import { isVisible } from "../../lib/appearance";
import { compactCanvas } from "../model/compactCanvas";
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
  const compaction = useMemo(
    () =>
      height > 0 && pipeline
        ? compactCanvas(pipeline, { withRuns: new Set(JSON.parse(withRuns) as string[]), card: { width, height } })
        : null,
    [width, height, pipeline, withRuns],
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
    const views = new Map(compaction?.pipeline.nodes.map((n) => [n.id, n.view]));
    return cards.map((card) => {
      const box = compaction?.cards[card.id];
      if (box) return { ...card, position: { x: box.x, y: box.y }, data: { ...card.data, runCanvas: true, ...(compact ? { compact: true } : {}) } };
      const view = views.get(card.id);
      return { ...card, ...(view ? { position: { x: view.x, y: view.y } } : {}), data: { ...card.data, runCanvas: true } };
    });
  }, [compaction, grown, cards, runCanvas, compact]);
  const shown = compaction?.pipeline ?? grown?.pipeline ?? pipeline;
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
