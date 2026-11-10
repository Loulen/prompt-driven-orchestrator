/**
 * What a tour step may light **beside** its target, and what a tour puts away
 * on its way out (#911, spec #910, ADR-0071 §4) — types only.
 *
 * The *Overview* tour opens panels with its gestures (the node inspector, the
 * edge panel, Start's input) and lets the reader look around in them; these are
 * the declarative fields a step uses for that. Its intro also installs an
 * example Trigger only to show it, and {@link TourTidyUp} is how a tour says it
 * must go. `lib/tourDef.ts` composes them into `TourStep` and `TourDef`.
 */

import type { RunStatus } from "../shared/generated/RunStatus";
import type { TourObservation } from "./tourDef";

/** One side bubble of a step (#911). */
export interface TourAside {
  /** CSS selector of what the bubble points at. */
  target: string;
  /** One sentence, two at most. */
  text: string;
}

/**
 * A card illustration (#911). One shape so far — a legend of the Status icon:
 * one row per status, led by the icon the real list draws (`StatusIcon`), so the
 * picture and the list cannot disagree. The shape carries the status, the colour
 * only reinforces it (CONTEXT.md § « Status icon »).
 */
export interface TourIllustration {
  kind: "rows";
  rows: {
    /** The Run status whose icon leads the row. */
    status: RunStatus;
    /** The shape and what it means, in the reader's words: « a check: finished ». */
    label: string;
  }[];
}

/** The fields a {@link import("./tourDef").TourStep} uses to show a panel (#911). */
export interface TourStepPanels {
  /**
   * A small picture inside the card, under the body — for the one step whose
   * subject cannot be shown by lighting the app (#911, after the human test): the
   * Status icon has several shapes and the tour's Run only ever wears one. Data,
   * not markup, so the popover renders it without knowing which step asked.
   */
  illustration?: TourIllustration;
  /**
   * Secondary lit areas, **read-only** (#911, after the human test): the panel
   * a gesture just opened stays bright and explorable — tabs, scroll, folds —
   * but nothing in it can be changed while the step is current. The step's own
   * target keeps every click. Resolved from the observation because most of these
   * panels only exist once the gesture they follow has been made; a selector
   * that resolves to nothing is simply not lit.
   */
  readOnly?: (o: TourObservation) => string[];
  /**
   * Controls inside a {@link readOnly} area that only *navigate* and stay
   * clickable: an inspector's tab buttons, a « show the prompt » fold. The engine
   * already lets `role="tab"`, `<summary>` and plain disclosures through; this is
   * for the app's own buttons that carry none of those markers.
   */
  explore?: string[];
  /**
   * Side bubbles (#911): once the step's condition holds, short notes pinned to
   * other parts of what the gesture opened — the Run's prompt in the Start
   * inspector, the condition in the edge panel. Not part of the flow: no Next,
   * no progress, and a target that is not on screen is skipped rather than
   * waited for.
   */
  asides?: (o: TourObservation) => TourAside[];
}

/**
 * The mirror of a `TourPreparation` (#911, ADR-0071 §4): something the
 * preparation installed **only to be shown** — the example Trigger — and that
 * does not belong to the user once the tour is over. Played **once**, at any exit
 * of the tour: its end (a Full tour chaining on included), a quit, a clean stop.
 *
 * Idempotent like a preparation: a tab closed mid-tour skips it, and the next
 * pass picks the object up by its name before putting it away.
 */
export interface TourTidyUp {
  id: string;
  /** Headline when it fails, e.g. "The example Trigger could not be removed". */
  failureTitle: string;
  /** Rejects with the daemon's own sentence, quoted on the card showing. */
  run: () => Promise<void>;
}
