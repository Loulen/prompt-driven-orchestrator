import type {
  TourAppState,
  TourDef,
  TourObservation,
  TourStep,
} from "./tourDef";
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

/** Resolve a step's body, which may depend on what the tour is observing (#825). */
export function stepBody(step: TourStep, o: TourObservation): string {
  return typeof step.body === "function" ? step.body(o) : step.body;
}

/** Resolve a step's quieter second paragraph, or `null` when it has none. */
export function stepNote(step: TourStep, o: TourObservation): string | null {
  if (!step.note) return null;
  return typeof step.note === "function" ? step.note(o) : step.note;
}

/** Is the step's lit area a zone to roam in **right now** (#825)? */
export function stepSoft(step: TourStep, o: TourObservation): boolean {
  return typeof step.soft === "function" ? step.soft(o) : step.soft === true;
}

/** Resolve the two end-card fields, which a tour may make depend on what it saw. */
export function recapIntroOf(tour: TourDef, app: TourAppState): string {
  return typeof tour.recapIntro === "function" ? tour.recapIntro(app) : tour.recapIntro;
}

export function recapOf(tour: TourDef, app: TourAppState): { label: string; text: string }[] {
  return typeof tour.recap === "function" ? tour.recap(app) : tour.recap;
}

export type TourPhase = "intro" | "running" | "failed" | "finished";

export interface TourFailure {
  stepId: string;
  /** 1-based, so the card can say "step 15 of 33" without arithmetic. */
  stepNumber: number;
  total: number;
  /**
   * `missing` — the target never appeared, and the tour cannot go on.
   * `refused` — the app said no, out loud; `reason` is its words (#824).
   */
  kind: "missing" | "refused";
  waitingFor: string;
  hint?: string;
  /** Only on a refusal: the app's own sentence, quoted rather than paraphrased. */
  reason?: string;
  /** Only on a refusal: the headline that replaces "This step could not be completed". */
  title?: string;
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
  /**
   * The app state the machine last read. The end card needs it *after* the tour
   * has stopped ticking, and by then the modal that held the Run's name is closed
   * — so the recap reads this rather than a live observation.
   */
  observedApp: TourAppState;
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
export function needsConfirm(tour: TourDef, run: TourRun, o: TourObservation): boolean {
  const step = currentStep(tour, run);
  if (!step) return false;
  const confirm = typeof step.confirm === "function" ? step.confirm(o) : !!step.confirm;
  return confirm || !step.done || run.satisfiedOnEntry;
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
    observedApp: o.app,
  };
}

/**
 * Start the tour. A tour with an {@link TourIntro} opens on its card instead of
 * its first step — the card is where its preparations run, and where `Start` is.
 */
export function startTour(tour: TourDef, o: TourObservation): TourRun {
  if (!tour.intro) return enterStep(tour, 0, o);
  return {
    tourId: tour.id,
    phase: "intro",
    index: 0,
    missingSince: null,
    satisfiedOnEntry: false,
    failure: null,
    observedApp: o.app,
  };
}

/** The intro card's `Start`: leave the card and enter the first step. */
export function beginSteps(tour: TourDef, run: TourRun, o: TourObservation): TourRun {
  if (run.phase !== "intro") return run;
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

/**
 * The `Skip` button. Only offered on a `skippable` step; never fills anything in.
 *
 * A step may declare that skipping it ends the tour (#825): the reader lands on
 * the recap, which says what it actually observed — including that the node is
 * still running.
 */
export function skipStep(tour: TourDef, run: TourRun, o: TourObservation): TourRun {
  const step = currentStep(tour, run);
  if (!step?.skippable) return run;
  if (step.skipEndsTour) return enterStep(tour, tour.steps.length, o);
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
  if (!step) return { ...run, phase: "finished", observedApp: o.app };

  // A refusal outranks everything, the condition included (#824). The app is
  // saying no in prose; a tour that walked on because some other predicate went
  // true would be talking over it.
  const refusal = step.refused?.(o) ?? null;
  if (refusal) {
    return {
      ...run,
      phase: "failed",
      observedApp: o.app,
      failure: {
        stepId: step.id,
        stepNumber: run.index + 1,
        total: tour.steps.length,
        kind: "refused",
        waitingFor: step.waitingFor,
        hint: step.refusalHint,
        reason: refusal,
        title: step.refusalTitle,
      },
    };
  }

  // `observedApp` is refreshed on transitions only (`enterStep`, and the two
  // failure branches), never on every tick: the state object is rebuilt each time
  // it is read, so copying it here would make `observeTour` return a new run ten
  // times a second and re-render the whole tour layer with it.

  if (isStepDone(step, o)) {
    const cleared = run.missingSince === null ? run : { ...run, missingSince: null };
    // Done, but this step hands the moment to the user (free input, acknowledge,
    // already-satisfied). It waits for `Next` — and, its condition being met, it
    // is no longer waiting for its target either.
    if (needsConfirm(tour, cleared, o)) return cleared;
    return goNext(tour, cleared, o);
  }

  const selectors = step.target(o);
  const present = selectors.length > 0 && selectors.every((s) => o.present(s));
  if (present) return run.missingSince === null ? run : { ...run, missingSince: null };

  const timeout = step.targetTimeoutMs === undefined ? DEFAULT_TARGET_TIMEOUT_MS : step.targetTimeoutMs;
  const missingSince = run.missingSince ?? now;
  if (timeout !== null && now - missingSince >= timeout) {
    return {
      ...run,
      phase: "failed",
      missingSince,
      observedApp: o.app,
      failure: {
        stepId: step.id,
        stepNumber: run.index + 1,
        total: tour.steps.length,
        kind: "missing",
        waitingFor: step.waitingFor,
        hint: step.failureHint,
      },
    };
  }
  return run.missingSince === missingSince ? run : { ...run, missingSince };
}
