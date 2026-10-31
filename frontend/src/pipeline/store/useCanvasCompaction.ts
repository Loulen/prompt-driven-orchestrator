import { useMemo } from "react";
import type { Node } from "@xyflow/react";
import { useCompactNodes } from "../../hooks/useCompactNodes";
import { compactCanvas } from "../model/compactCanvas";
import type { EditorPipelineDef } from "../model/pipeline";
import { isRunTabLocked, type OpenPipeline } from "./editState";

/**
 * « Compact nodes »: every Run canvas that is not being edited draws its cards
 * on one line, from a shifted copy of the pipeline; nothing is written. A Run
 * edited for itself, like the editor, keeps the detailed cards: a drag there
 * writes positions. `shown` is the pipeline to draw, `placed` the derived
 * cards at their compact place.
 */
export function useCanvasCompaction(tab: OpenPipeline | undefined, pipeline: EditorPipelineDef | undefined, cards: Node[]) {
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
  const placed = useMemo(() => {
    if (!compaction) return cards;
    const views = new Map(compaction.pipeline.nodes.map((n) => [n.id, n.view]));
    return cards.map((card) => {
      const box = compaction.cards[card.id];
      if (box) return { ...card, position: { x: box.x, y: box.y }, data: { ...card.data, compact: true } };
      const view = views.get(card.id);
      return view ? { ...card, position: { x: view.x, y: view.y } } : card;
    });
  }, [compaction, cards]);
  return { shown: compaction?.pipeline ?? pipeline, cards: compaction?.cards, placed };
}
