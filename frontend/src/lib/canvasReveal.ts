/**
 * « Bring this canvas node into view » — the canvas's answer to `scrollIntoView`.
 *
 * A guided tour already puts its target in front of the reader when the page has
 * scrolled away from it. The canvas cannot be reached that way: a node is an
 * absolutely-positioned card inside a transformed viewport, so `scrollIntoView`
 * on it scrolls nothing, and a target outside the visible canvas is a step the
 * reader can neither see nor satisfy — the overlay owns the wheel and the drag,
 * so there is no panning out of it either. That is how the *First pipeline* tour
 * dead-ended on « click the new card » (#825, FP iteration 2).
 *
 * Placement is the first defence (`lib/nodePlacement.ts` keeps a new card on
 * screen); this is the second, for a card the reader panned away from, or one
 * that was already there. Built like `lib/overlays.ts`: the canvas registers what
 * it can do while it is mounted, and knows nothing about tours (« aucune logique
 * de tour dans les composants existants », #823).
 */

/** Pan the canvas so the node with this id is in the middle of it. */
type Reveal = (nodeId: string) => void;

let reveal: Reveal | null = null;

/**
 * Declare this canvas able to show a node. Returns the unregister, so the
 * canvas's effect can simply `return registerCanvasReveal(fn)`.
 *
 * One canvas at a time: only the active tab's is mounted, and a later one
 * replacing an earlier one is the truth of what is on screen.
 */
export function registerCanvasReveal(fn: Reveal): () => void {
  reveal = fn;
  return () => {
    if (reveal === fn) reveal = null;
  };
}

/**
 * Ask the canvas to bring a node into view. A no-op when no canvas is mounted —
 * the caller is describing a wish, not driving a component.
 */
export function revealCanvasNode(nodeId: string): void {
  reveal?.(nodeId);
}

/**
 * The node id of a canvas card, or `null` for anything else. xyflow stamps
 * `data-id` on `.react-flow__node`, which is what every canvas target selector
 * in `lib/tours/` is built on.
 */
export function canvasNodeIdOf(element: Element): string | null {
  const card = element.closest(".react-flow__node");
  return card?.getAttribute("data-id") ?? null;
}
