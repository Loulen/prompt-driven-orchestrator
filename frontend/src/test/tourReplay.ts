/**
 * The replay probe of the Rejeu d'étape (CONTEXT.md § « Rejeu d'étape »), shared
 * by the three tours' content tests.
 *
 * « Previous never stops the tour on a failure card ». Called at a step of a
 * tour's walk, it plays what a reader does with Previous from there, without
 * touching the app, since the machine is pure and the walk goes on from the run
 * it was handed:
 *
 * - Previous greyed out is an answer too: nothing to check.
 * - The replayed step is held for longer than any step's timeout, as a reader
 *   rereading it would. It must not stop, nor slide away by itself.
 * - If it waits for `Next` (its condition holds), `Next` must lead back to the
 *   step Previous was pressed on, and that one must hold too.
 * - If it is live (its condition no longer holds), it is live on its target:
 *   the reader's own gesture moves on from there, as on the first pass.
 *
 * Returns what went wrong, in words, or `null`, so the probe imports no test
 * runner and each tour asserts with its own message.
 */
import {
  canGoBack,
  confirmStep,
  currentStep,
  goBack,
  needsConfirm,
  observeTour,
  targetOnScreen,
  type TourRun,
} from "../lib/tour";
import type { TourDef, TourObservation } from "../lib/tourDef";

/** Longer than every step's timeout, the reading steps' fifteen seconds included. */
const HOLD_MS = 30_000;
const TICK_MS = 1_000;

/** What a replay looked like, for the tests that also check its shape. */
export interface ReplayOutcome {
  /** Previous was offered from this step. */
  offered: boolean;
  /**
   * The replayed step came back with its target off screen: replayable on its
   * condition alone. Each tour pins the list, so a step whose instruction needs
   * that screen gets `replayNeedsTarget` by decision, not by luck.
   */
  offScreen: boolean;
  /** What went wrong, or `null`. */
  problem: string | null;
}

/** Hold the run for {@link HOLD_MS}, as the host's interval would. */
function hold(tour: TourDef, run: TourRun, o: TourObservation, from: number): TourRun {
  let r = run;
  for (let t = from; t <= from + HOLD_MS && r.phase === "running"; t += TICK_MS) {
    r = observeTour(tour, r, o, t);
  }
  return r;
}

export function probeReplay(tour: TourDef, run: TourRun, o: TourObservation, now: number): ReplayOutcome {
  const here = currentStep(tour, run)?.id;
  if (run.phase !== "running" || !here || !canGoBack(tour, run, o)) {
    return { offered: false, offScreen: false, problem: null };
  }

  const offScreen = !targetOnScreen(tour.steps[run.index - 1]!, o);
  const back = goBack(tour, run, o);
  const replayed = currentStep(tour, back)?.id;
  const expected = tour.steps[run.index - 1]?.id;
  if (replayed !== expected) {
    return { offered: true, offScreen, problem: `Previous from "${here}" went to "${replayed}"` };
  }

  const held = hold(tour, back, o, now);
  if (held.phase === "failed") {
    return {
      offered: true,
      offScreen,
      problem: `replaying "${replayed}" from "${here}" stopped the tour (${held.failure?.kind})`,
    };
  }
  if (currentStep(tour, held)?.id !== replayed) {
    return {
      offered: true,
      offScreen,
      problem: `replaying "${replayed}" from "${here}" slid on to "${currentStep(tour, held)?.id}" by itself`,
    };
  }

  if (!needsConfirm(tour, held, o)) {
    const live = targetOnScreen(currentStep(tour, held)!, o);
    return {
      offered: true,
      offScreen,
      problem: live ? null : `replaying "${replayed}" from "${here}" left it live on a missing target`,
    };
  }

  const forward = confirmStep(tour, held, o);
  if (currentStep(tour, forward)?.id !== here) {
    return {
      offered: true,
      offScreen,
      problem: `Next on the replayed "${replayed}" went to "${currentStep(tour, forward)?.id}", not back to "${here}"`,
    };
  }
  const after = hold(tour, forward, o, now + HOLD_MS);
  if (after.phase === "failed") {
    return {
      offered: true,
      offScreen,
      problem: `"${here}", re-entered after replaying "${replayed}", stopped the tour (${after.failure?.kind})`,
    };
  }
  return { offered: true, offScreen, problem: null };
}
