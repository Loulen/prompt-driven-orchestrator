import { useEffect, useMemo } from "react";
import type { Node } from "@xyflow/react";
import { useCompactNodes } from "../../hooks/useCompactNodes";
import { compactCanvas } from "../model/compactCanvas";
import { grownCanvas } from "../model/grownCanvas";
import { IDENTITY_SHIFT, type CanvasShift } from "./canvasShift";
import type { EditorPipelineDef } from "../model/pipeline";
import { isRunTabLocked, type OpenPipeline } from "./editState";

/**
 * « Compact nodes »: every Run canvas that is not being edited draws its cards
 * on one line, from a shifted copy of the pipeline; nothing is written. A Run
 * edited for itself, like the editor, keeps the detailed cards: a drag there
 * writes positions. `shown` is the pipeline to draw, `placed` the derived
 * cards at their compact place.
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
  const { compactNodes } = useCompactNodes();
  const compactOn = compactNodes && isRunTabLocked(tab);
  // The orchestrators with child runs carry « N runs » on both sizes: their
  // ids as one string, so a new array with the same content changes nothing.
  const withRuns = useMemo(
    () => JSON.stringify(cards.filter((c) => (c.data as { childCounts?: unknown }).childCounts != null).map((c) => c.id)),
    [cards],
  );
  const compaction = useMemo(
    () => (compactOn && pipeline ? compactCanvas(pipeline, { withRuns: new Set(JSON.parse(withRuns) as string[]) }) : null),
    [compactOn, pipeline, withRuns],
  );
  const growth = useMemo(
    () => (!compactOn && template && pipeline ? grownCanvas(pipeline) : null),
    [compactOn, template, pipeline],
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
    if (!compaction) return cards;
    const views = new Map(compaction.pipeline.nodes.map((n) => [n.id, n.view]));
    return cards.map((card) => {
      const box = compaction.cards[card.id];
      if (box) return { ...card, position: { x: box.x, y: box.y }, data: { ...card.data, compact: true } };
      const view = views.get(card.id);
      return view ? { ...card, position: { x: view.x, y: view.y } } : card;
    });
  }, [compaction, grown, cards]);
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
