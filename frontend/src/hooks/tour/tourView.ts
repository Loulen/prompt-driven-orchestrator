/**
 * The shapes the tour hook hands to the tour layer (#823): the rectangle the
 * Projecteur draws with, the live view of the running step, the intro card's
 * preparation state, and the controller `useTour` returns.
 */

import type { TourRun } from "../../lib/tour";
import type { TourChecklistItem, TourDef, TourStep } from "../../lib/tourDef";

/** Viewport-space rectangle, the shape the Projecteur draws with. */
export interface TourRect {
  top: number;
  left: number;
  width: number;
  height: number;
}

export interface TourView {
  tour: TourDef;
  run: TourRun;
  step: TourStep | null;
  /** 1-based, for "Step n of N". */
  stepNumber: number;
  total: number;
  /** Union of the step's targets, padded — the hole in the dim. */
  hole: TourRect | null;
  /**
   * The soft area the hole sits in, drawn dashed: the open menu an option lives
   * in, or — when the step's own target is the zone (#825) — the hole itself.
   */
  zone: TourRect | null;
  /** The zone IS the target: the dim lightens and no ring is drawn (#825). */
  wideZone: boolean;
  /**
   * The step's read-only areas, lit beside the hole (#911) — the panel its
   * gesture opened. Empty when it declares none, or none is on screen.
   */
  lit: TourRect[];
  /** The step's side bubbles whose target is on screen, once its condition holds (#911). */
  asides: TourAsideView[];
  /** The step's condition holds: `Next` is enabled. */
  ready: boolean;
  /** The step waits for `Next` instead of advancing on its own. */
  awaitingConfirm: boolean;
  /** The step's body, resolved against what the tour is observing (#825). */
  body: string;
  /** The step's quieter second paragraph, resolved the same way. */
  note: string | null;
  /** The step's live checklist, recomputed each tick. Empty when it declares none. */
  checklist: TourChecklistItem[];
  /** The intro card's state, while `run.phase === "intro"` (#824). */
  prep: TourPrepView | null;
  /**
   * The tour's teardown failed (#911): said on the card showing — the end card
   * or the stop card — never swallowed. `null` while nothing went wrong.
   */
  tidyUpFailure: { title: string; reason: string } | null;
  /**
   * The next leg of a **Full tour**, or `null` for a tour started on its own
   * (#824). The end card's primary chains to it instead of closing — that is the
   * whole difference between "the Full tour" and "two tours you start by hand".
   */
  nextTour: TourDef | null;
}

/** One side bubble, resolved to where its target is (#911). */
export interface TourAsideView {
  /** The aside's selector — stable across ticks, so React keeps the bubble. */
  target: string;
  text: string;
  rect: TourRect;
}

/** What the intro card shows while it prepares what the tour needs (#824). */
export interface TourPrepView {
  items: { id: string; label: string; done: boolean }[];
  /** Every preparation has answered: `Start` is live. */
  ready: boolean;
  /** One refused. The card swaps to its error state and the tour never starts. */
  failure: { title: string; reason: string } | null;
}

export interface TourController {
  /** The live view, or null when no tour is running. */
  view: TourView | null;
  /**
   * Start a tour. `rest` is the remaining legs of a Full tour: the end card then
   * chains into the next one rather than offering a way out.
   */
  start: (tour: TourDef, rest?: TourDef[]) => void;
  /** Leave without marking anything: an abandoned tour is not a finished one. */
  quit: () => void;
  next: () => void;
  skip: () => void;
  /** The end card's primary — the only thing that writes the "done" key. */
  finish: () => void;
  /**
   * « Finish here » on the intermediate card of a Full tour (#825): this tour is
   * done, the chain is not continued. The checkmark is earned either way — what
   * stops is the sequence, not the achievement.
   */
  finishHere: () => void;
  /**
   * « Continue · <next> » on the card of a tour that STOPPED (#825). Marks
   * nothing: a tour that could not finish has not been done. The chain survives
   * the stop, because a refused Launch says something about the machine, not
   * about the tour that comes next.
   */
  continueChain: () => void;
  /** The intro card's `Start`: enter the first step. */
  begin: () => void;
  /** The intro card's `Retry` after a refused preparation. */
  retryPrep: () => void;
}
