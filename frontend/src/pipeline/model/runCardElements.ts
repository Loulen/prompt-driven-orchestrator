/**
 * The Éléments masquables of a node's card on a Run canvas (surface
 * `run.canvas`): what a switch of Settings › Appearance hides. Its type icon,
 * its name, its status and an orchestrator's child runs are fixed. They apply
 * on a Run canvas that is not being edited; the pipeline editor and a Run
 * edited for itself keep every element.
 *
 * The cells of the meta grid pack two per row, so the card's height follows
 * how many of them show ({@link runCardSize}): the canvas then rises by the
 * difference (`compactCanvas.ts`), as « Compact nodes » does.
 */
import type { RowElement } from "../../run/model/runRowElements";
import { COMPACT_CARD, type CardSize } from "./compactCanvas";

export const RUN_CARD = {
  agent: "run.canvas.agent",
  iteration: "run.canvas.iteration",
  elapsed: "run.canvas.elapsed",
  cost: "run.canvas.cost",
  isolation: "run.canvas.isolation",
  loop: "run.canvas.loop",
  profile: "run.canvas.profile",
} as const;

/** The meta grid's cells, in their order. */
export const RUN_CARD_CELLS: readonly RowElement[] = [
  { id: RUN_CARD.agent, label: "Model" },
  { id: RUN_CARD.iteration, label: "Iteration" },
  { id: RUN_CARD.elapsed, label: "Elapsed time" },
  { id: RUN_CARD.cost, label: "Cost" },
];

/** The header's markers. */
export const RUN_CARD_MARKERS: readonly RowElement[] = [
  { id: RUN_CARD.isolation, label: "Isolation flag" },
  { id: RUN_CARD.loop, label: "Loop badge" },
  { id: RUN_CARD.profile, label: "Agent profile marker" },
];

/** Every switch of the card, in the block's order. */
export const RUN_CARD_ELEMENTS: readonly RowElement[] = [...RUN_CARD_CELLS, ...RUN_CARD_MARKERS];

/**
 * The heights of a Run card, in flow px, as `canvas.css` draws them: change
 * both together. Its 24 px header and its border; the meta grid adds its
 * padding (7 px) and one 16 px row per two cells.
 */
export const RUN_CARD_HEIGHT = { head: 26, gridPad: 7, row: 16 } as const;

/** The drawn height of a Run card showing `cells` cells of its meta grid (an orchestrator's runs row aside). */
export function runCardHeight(cells: number): number {
  const rows = Math.ceil(cells / 2);
  return RUN_CARD_HEIGHT.head + (rows > 0 ? RUN_CARD_HEIGHT.gridPad + RUN_CARD_HEIGHT.row * rows : 0);
}

/**
 * The size of a work card on a Run canvas, from the Apparence alone (`shown`
 * says whether an element id shows): one line with « Compact nodes », else
 * the full width at the height of its visible cells. `null` while every cell
 * shows: the card is the detailed one every position was placed for, and the
 * canvas is drawn as stored.
 */
export function runCardSize(shown: (id: string) => boolean, compactNodes: boolean): CardSize | null {
  if (compactNodes) return { width: COMPACT_CARD.compactWidth, height: COMPACT_CARD.compact };
  const height = runCardHeight(RUN_CARD_CELLS.filter((cell) => shown(cell.id)).length);
  return height === COMPACT_CARD.detailed ? null : { width: COMPACT_CARD.width, height };
}
