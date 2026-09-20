/**
 * The tour machine (#823, spec #821, ADR-0071) — pure, DOM-free.
 *
 * A **Tour** (CONTEXT.md § « Tours guidés ») is a list of declarative steps. This
 * module owns the part that can be reasoned about without a browser: which step is
 * current, when it advances, when the tour stops because its target never showed up,
 * and what quitting does. Everything that needs a DOM — resolving a selector to a
 * rectangle, drawing the Projecteur, scrolling a target into view — lives in
 * `hooks/useTour.ts`, and every piece of content lives in `lib/tours/`.
 *
 * Two rules from ADR-0071 shape the whole design:
 *
 * 1. **A step advances on observed state, never on a delay.** The machine is handed a
 *    {@link TourObservation} and asks the step's own `done` predicate. There is no
 *    `setTimeout(next)` anywhere: a timer only ever *fails* a step whose target never
 *    appeared, and even that is a real observation (the selector resolves to nothing).
 * 2. **The tour never acts for the user.** Nothing here writes to the app. `skip`
 *    moves on and leaves the form exactly as the user left it.
 */

import type { PipelineDef } from "../types";

/**
 * The slice of app state a step's `done` predicate may read — all of it already
 * loaded by the UI for its own sake (ADR-0071 §3: the tour observes, it never
 * fetches). The hook fills this from the edit store and the run list.
 */
export interface TourAppState {
  /** The active edit tab's id (the pipeline's YAML file stem), or null. */
  pipelineId: string | null;
  /** The active edit tab's pipeline document. */
  pipeline: PipelineDef | null;
  /** Node prompts, which the edit store keeps beside the document. */
  prompts: Record<string, string>;
  selection: { kind: string; id: string | null; edgeIndex: number | null };
  /** The active tab has unsaved changes. */
  dirty: boolean;
  /** Ids of the pipelines the Library currently lists. */
  libraryPipelineIds: string[];
  /** How many Runs this instance has (the welcome-modal rule reads it too). */
  runCount: number;
}

/**
 * What a step is allowed to look at. `present` / `value` are the DOM, narrowed to
 * two questions, so the predicates stay unit-testable with plain stubs.
 */
export interface TourObservation {
  app: TourAppState;
  /** Does this CSS selector resolve right now? */
  present: (selector: string) => boolean;
  /**
   * The live value of a form control, **uncommitted**. Several fields in the
   * inspector only write to the store on blur (`NameInput`), so a step that waited
   * for the committed value could never enable its own `Next` — the user would have
   * to blur the field first, which is precisely the gesture `Next` is meant to be.
   */
  value: (selector: string) => string | null;
}

export interface TourStep {
  id: string;
  /** Imperative — the one thing to do. */
  title: string;
  /** Two sentences max (CONTEXT.md § « Étape de tour »). */
  body: string;
  /**
   * The step's target(s), resolved from the observed state because most of them
   * only exist once the user has created the node they point at. The Projecteur's
   * hole is the union of their bounding boxes; an empty list means "missing", which
   * is what eventually stops the tour.
   */
  target: (app: TourAppState) => string[];
  /** How the failure card names what the tour was waiting for. */
  waitingFor: string;
  /** One extra sentence on the failure card, in the user's terms. */
  failureHint?: string;
  /** A block to paste, rendered with a Copy button. Any free input gets one. */
  copyBlock?: string;
  /**
   * Free input: the step shows `Next` (enabled once `done` holds) instead of
   * advancing on its own, so the user owns the moment they move on (story 14).
   */
  confirm?: boolean;
  /** Optional step — the popover offers `Skip` (story 15). */
  skippable?: boolean;
  /**
   * How long the target may stay missing before the tour stops, in ms. `null`
   * waits forever (the *First run* tour's "wait for the node to finish").
   */
  targetTimeoutMs?: number | null;
  /**
   * The target lives in an open menu (a portal). The Projecteur then draws a soft,
   * dashed zone over the whole menu and keeps every option clickable — a tour must
   * point at the right one, not forbid the others.
   */
  soft?: boolean;
  /**
   * The observed condition that completes the step. **Absent means "acknowledge"**:
   * there is nothing to observe, so the popover shows an always-enabled `Next`.
   */
  done?: (o: TourObservation) => boolean;
}

export interface TourDef {
  id: string;
  /** Name in the UI, e.g. "First pipeline". */
  title: string;
  /** One line, for the welcome modal and Settings › Tutorials. */
  blurb: string;
  /** Rough duration in minutes, shown next to Start. */
  minutes: number;
  steps: TourStep[];
  /** End card headline, e.g. "You built …". */
  recapIntro: string;
  /** End card bullets: what the thing the user just built actually does. */
  recap: { label: string; text: string }[];
}

export type TourPhase = "running" | "failed" | "finished";

export interface TourFailure {
  stepId: string;
  /** 1-based, so the card can say "step 15 of 33" without arithmetic. */
  stepNumber: number;
  total: number;
  waitingFor: string;
  hint?: string;
}

export interface TourRun {
  tourId: string;
  phase: TourPhase;
  /** 0-based index into `TourDef.steps`. */
  index: number;
  /** When the target first went missing *during this step*, else null. */
  missingSince: number | null;
  /**
   * The step's condition already held the moment it was entered — the field was
   * already set to what the tour is about to ask for (a new agent node is born
   * named `implementer` and isolated, so two steps of *First pipeline* land here).
   * Auto-advancing would flash past the explanation, which is the only thing those
   * steps have to give, so the popover asks for an explicit `Next` instead.
   */
  satisfiedOnEntry: boolean;
  failure: TourFailure | null;
}

/** A missing target stops the tour after this long, unless the step says otherwise. */
export const DEFAULT_TARGET_TIMEOUT_MS = 5_000;

export function currentStep(tour: TourDef, run: TourRun): TourStep | null {
  return tour.steps[run.index] ?? null;
}

/** Is the step's condition satisfied right now? Drives `Next`'s enabled state. */
export function isStepDone(step: TourStep, o: TourObservation): boolean {
  return step.done ? step.done(o) : false;
}

/**
 * May the user press `Next` / may the step advance on its own?
 *
 * An acknowledge step (no `done`) and a step satisfied on entry are always ready:
 * there is nothing left to observe, only something to read.
 */
export function canAdvance(tour: TourDef, run: TourRun, o: TourObservation): boolean {
  const step = currentStep(tour, run);
  if (!step) return false;
  if (!step.done || run.satisfiedOnEntry) return true;
  return step.done(o);
}

/** Does the popover show `Next` rather than advancing by itself? */
export function needsConfirm(tour: TourDef, run: TourRun): boolean {
  const step = currentStep(tour, run);
  if (!step) return false;
  return !!step.confirm || !step.done || run.satisfiedOnEntry;
}

function enterStep(tour: TourDef, index: number, o: TourObservation): TourRun {
  const step = tour.steps[index];
  return {
    tourId: tour.id,
    phase: step ? "running" : "finished",
    index,
    missingSince: null,
    satisfiedOnEntry: step ? isStepDone(step, o) : false,
    failure: null,
  };
}

export function startTour(tour: TourDef, o: TourObservation): TourRun {
  return enterStep(tour, 0, o);
}

/** Move to the next step (or finish). Shared by auto-advance, `Next` and `Skip`. */
function goNext(tour: TourDef, run: TourRun, o: TourObservation): TourRun {
  if (run.phase !== "running") return run;
  return enterStep(tour, run.index + 1, o);
}

/** The `Next` button. Refuses while the step's condition is not met. */
export function confirmStep(tour: TourDef, run: TourRun, o: TourObservation): TourRun {
  if (!canAdvance(tour, run, o)) return run;
  return goNext(tour, run, o);
}

/** The `Skip` button. Only offered on a `skippable` step; never fills anything in. */
export function skipStep(tour: TourDef, run: TourRun, o: TourObservation): TourRun {
  const step = currentStep(tour, run);
  if (!step?.skippable) return run;
  return goNext(tour, run, o);
}

/**
 * One tick. The hook calls this on an interval and on every scroll/resize; it is
 * pure, so calling it more often only costs a comparison.
 *
 * Order matters, and it is the opposite of the intuitive one: **the condition is
 * read before the target's presence**. Most gestures a tour teaches DESTROY their
 * own target — clicking Create closes the dialog, picking Node closes the menu — so
 * a machine that checked presence first would start a "this step could not be
 * completed" countdown at the exact moment the user got it right. A missing target
 * only means something while the step is still unsatisfied.
 */
export function observeTour(tour: TourDef, run: TourRun, o: TourObservation, now: number): TourRun {
  if (run.phase !== "running") return run;
  const step = currentStep(tour, run);
  if (!step) return { ...run, phase: "finished" };

  if (isStepDone(step, o)) {
    const cleared = run.missingSince === null ? run : { ...run, missingSince: null };
    // Done, but this step hands the moment to the user (free input, acknowledge,
    // already-satisfied). It waits for `Next` — and, its condition being met, it
    // is no longer waiting for its target either.
    if (needsConfirm(tour, cleared)) return cleared;
    return goNext(tour, cleared, o);
  }

  const selectors = step.target(o.app);
  const present = selectors.length > 0 && selectors.every((s) => o.present(s));
  if (present) return run.missingSince === null ? run : { ...run, missingSince: null };

  const timeout = step.targetTimeoutMs === undefined ? DEFAULT_TARGET_TIMEOUT_MS : step.targetTimeoutMs;
  const missingSince = run.missingSince ?? now;
  if (timeout !== null && now - missingSince >= timeout) {
    return {
      ...run,
      phase: "failed",
      missingSince,
      failure: {
        stepId: step.id,
        stepNumber: run.index + 1,
        total: tour.steps.length,
        waitingFor: step.waitingFor,
        hint: step.failureHint,
      },
    };
  }
  return run.missingSince === missingSince ? run : { ...run, missingSince };
}
