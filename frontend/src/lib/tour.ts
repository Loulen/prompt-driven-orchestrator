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
 * One node of the Run a tour is following (#825), narrowed to the two facts the
 * reading steps need: has its completion guard been released (ADR-0068), and has
 * it stopped. Both come from the state the Run inspector already polls — the tour
 * adds no request of its own (ADR-0071 §3).
 */
export interface TourRunNode {
  id: string;
  /** The name the Run's frozen pipeline gave it, else its id. */
  name: string;
  /** `agent`, `start`, `end`… — the reading steps only ever look for an agent. */
  type: string;
  /** `pending` | `running` | `awaiting_user` | `completed` | `failed` | … */
  status: string;
  /** The current iteration's completion guard is open (ADR-0068). */
  released: boolean;
}

/** Statuses a node no longer moves out of. `completed` is the happy one; the
 *  others still END the wait — a tour that kept spinning on a failed node would
 *  be lying about what it is waiting for. */
const TERMINAL_NODE_STATUSES = new Set(["completed", "failed", "skipped", "stopped"]);

export function isNodeFinished(node: TourRunNode | null): boolean {
  return node != null && TERMINAL_NODE_STATUSES.has(node.status);
}

/** The Run a tour is following — its list entry, plus the detail the inspector
 *  loaded once the user selected it (`nodes` stays empty until then). */
export interface TourRunView {
  id: string;
  name: string;
  nodes: TourRunNode[];
}

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
  /**
   * The newest Run in the list, or `null` on a Run-less instance (#824). The
   * *First run* tour ends on a recap of the Run the user just launched, and the
   * modal that held its name is closed by then — so the name has to come from the
   * list, which the UI has loaded anyway (ADR-0071 §3).
   *
   * `nodes` is empty until the user opens that Run: the detail is fetched by the
   * inspector, for the inspector. That is not a gap the tour has to paper over —
   * the step that asks the user to open the Run is precisely the one before the
   * steps that read its nodes (#825).
   */
  latestRun: TourRunView | null;
  /**
   * The Run whose tab is open on the canvas, or `null` on a pipeline tab (#825).
   * Read as « did the user open the Run we are talking about », which is one tab
   * id comparison away but reads as a question rather than a string prefix.
   */
  activeRunId: string | null;
}

/**
 * What a step is allowed to look at. `present` / `value` are the DOM, narrowed to
 * two questions, so the predicates stay unit-testable with plain stubs.
 */
export interface TourObservation {
  app: TourAppState;
  /**
   * The app state as it was the moment the tour started (#824). "Has something
   * appeared *because of this tour*" is not answerable from the present alone: an
   * instance with nine Runs already has `runCount > 0` before the user clicks
   * anything, so *First run*'s last step compares against this instead.
   */
  baseline: TourAppState;
  /** Does this CSS selector resolve right now? */
  present: (selector: string) => boolean;
  /**
   * The live value of a form control, **uncommitted**. Several fields in the
   * inspector only write to the store on blur (`NameInput`), so a step that waited
   * for the committed value could never enable its own `Next` — the user would have
   * to blur the field first, which is precisely the gesture `Next` is meant to be.
   */
  value: (selector: string) => string | null;
  /**
   * The rendered text of an element (#824). Read by a step that must quote the app
   * back at the user — the daemon's refusal under a Launch button is prose the
   * modal owns, and a tour that paraphrased it would be inventing a reason.
   */
  text: (selector: string) => string | null;
}

export interface TourStep {
  id: string;
  /** Imperative — the one thing to do. */
  title: string;
  /**
   * Two sentences max (CONTEXT.md § « Étape de tour »).
   *
   * A function when what there is to say depends on what happened: the wait step
   * of *First run* explains what to watch for while the node runs, and has
   * nothing left to instruct once it ended in failure (#825). Returning an empty
   * string renders no paragraph at all.
   */
  body: string | ((o: TourObservation) => string);
  /**
   * A second, quieter paragraph, under the body and the checklist. Two jobs, one
   * slot: naming the look-alike control the step is NOT about ("Mark complete …
   * not for now"), and saying what an observed accident means once the checklist
   * has shown it (a node that failed rather than completed) — #825.
   */
  note?: string | ((o: TourObservation) => string | null);
  /**
   * What ends the step, in the footer, where a step that advances on its own
   * would otherwise show an empty row: « advances once released », « advances on
   * its own » (#825). Never shown next to a `Next` button — the button says it.
   */
  advanceHint?: string;
  /**
   * A spinner row under the body while the step is unsatisfied (#825): the one
   * step with no time limit has to say so, or an unbounded wait reads as a freeze.
   */
  waitingNote?: string;
  /**
   * The step's target(s), resolved from the observation because most of them only
   * exist once the user has created the node they point at. The Projecteur's hole
   * is the union of their bounding boxes; an empty list means "missing", which is
   * what eventually stops the tour.
   *
   * It reads the DOM too, not just the app state, so **one** step can re-aim as
   * the user works: "choose `tutorial-interactive`" points at the menu's trigger
   * while the menu is shut and at the option once it is open (#824). Two steps for
   * that would spend a card on "now open this menu", and a tour is nine cards long
   * because each one is an idea.
   */
  target: (o: TourObservation) => string[];
  /** How the failure card names what the tour was waiting for. */
  waitingFor: string;
  /** One extra sentence on the failure card, in the user's terms. */
  failureHint?: string;
  /** A block to paste, rendered with a Copy button. Any free input gets one. */
  copyBlock?: string;
  /**
   * Free input: the step shows `Next` (enabled once `done` holds) instead of
   * advancing on its own, so the user owns the moment they move on (story 14).
   *
   * A predicate when only SOME outcomes are worth stopping on (#825): the wait
   * step slides on by itself when the node completes, and asks for a click when
   * it failed, because the sentence explaining the failure has to be read.
   */
  confirm?: boolean | ((o: TourObservation) => boolean);
  /** Optional step — the popover offers `Skip` (story 15). */
  skippable?: boolean;
  /**
   * `Skip` here ends the tour on its recap rather than moving to the next step
   * (#825, design Q3). For the one step that is a *wait*: skipping it means "I
   * have seen enough", and the steps after it are about a node that, by
   * definition, has not finished — the next one would point at an output file
   * nobody has written and stop the tour on a failure card.
   */
  skipEndsTour?: boolean;
  /**
   * How long the target may stay missing before the tour stops, in ms. `null`
   * waits forever (the *First run* tour's "wait for the node to finish").
   */
  targetTimeoutMs?: number | null;
  /**
   * The lit area is a **zone to roam in**, not a thing to hit: drawn dashed
   * instead of ringed, and — when the zone is the target itself — over a lighter
   * dim. Two shapes of the same idea:
   *
   * - the target lives in an open menu (a portal): the zone is the menu, so every
   *   option stays clickable. A tour points at the right one, it does not forbid
   *   the others.
   * - the target IS the zone (#825): the whole Run inspector, during a wait of
   *   unknown length. Read the terminal, scroll it, answer a late question — the
   *   one thing that step must not do is lock the reader out of the screen.
   *
   * A predicate when the target only *becomes* a zone: the last step of *First
   * run* rings the `out` row to be clicked, and lights the artifact that opens
   * as a page to read. Same step, two shapes, because « now read it » is not an
   * idea worth a card of its own.
   */
  soft?: boolean | ((o: TourObservation) => boolean);
  /**
   * The observed condition that completes the step. **Absent means "acknowledge"**:
   * there is nothing to observe, so the popover shows an always-enabled `Next`.
   */
  done?: (o: TourObservation) => boolean;
  /**
   * A live checklist under the body (#824), for the one step that asks for **two**
   * gestures at once (tick both PDO skills). Splitting it in two steps would spend
   * a whole card on "now tick the other one"; showing nothing would make the first
   * tick look like it did nothing. This says what happened.
   */
  checklist?: (o: TourObservation) => TourChecklistItem[];
  /**
   * Open the filesystem explorer at this path while the step is current (#824),
   * instead of at the field's own value. The tour cannot type for the user
   * (ADR-0071 §2), so an explorer that had to be navigated to `/tmp` by hand would
   * be four folder clicks of nothing.
   */
  explorerStart?: string;
  /**
   * The step is **refused** rather than merely unfinished: the app is showing a
   * reason the user must read (a daemon that would not launch the Run). Returns
   * that reason verbatim, or `null` while nothing is wrong.
   *
   * Checked before `done`, so a refusal cannot be raced past by a condition that
   * happens to hold. A refused tour stops on its own card and is **not** marked
   * done — the user's form is left exactly as they filled it.
   */
  refused?: (o: TourObservation) => string | null;
  /** Headline of the refusal card, e.g. "The Run could not be launched". */
  refusalTitle?: string;
  /** One sentence under the quoted reason, in the user's terms. */
  refusalHint?: string;
}

export interface TourChecklistItem {
  label: string;
  done: boolean;
  /**
   * Why this item is still open, and what to do about it (#825). The engine has
   * no « back » button, so the wait step's checklist is where a user who skipped
   * the release learns the node is waiting for them — the button is inside the
   * soft zone, so they can still press it.
   */
  note?: string;
  /** A badge next to a *done* item, when done is not the same as well: the node
   *  finished — `failed`. */
  badge?: string;
}

/**
 * One thing a tour needs to *exist* before its first step can point at anything
 * (#824) — a training repository, a pipeline in the Library. Run while the intro
 * card is on screen, in parallel, and every one of them must be **idempotent**:
 * replaying the tour has to be free, and a user who edited what a previous run
 * created keeps their edit.
 *
 * The daemon verbs behind these know nothing about tours (ADR-0071 §3); `run` is
 * where a tour's content binds one to its own arguments.
 */
export interface TourPreparation {
  id: string;
  /** Shown while it is in flight, e.g. "Pipeline tutorial-interactive in the Library…". */
  pending: string;
  /** Shown once it has answered, e.g. "Training repository /tmp/pdo-tutorial ready". */
  ready: string;
  /** Headline when THIS one refuses, e.g. "The training repository could not be created". */
  failureTitle: string;
  /** Idempotent. Rejects with the daemon's own sentence, which is quoted verbatim. */
  run: () => Promise<void>;
}

/**
 * The card a tour opens on, before any target is lit (#824). It is where a tour
 * says what it is about to do, and — when it has preparations — where it waits
 * for them: `Start` stays dead until every one has answered, because a tour that
 * began half-prepared would fail on a step whose target was never created.
 */
export interface TourIntro {
  /** Headline, e.g. "Launch your first Run". */
  title: string;
  /** What the tour will do, in the user's terms. */
  body: string;
  /** The reassurance under the checklist ("Nothing here touches your own repositories."). */
  footnote: string;
  prepare: TourPreparation[];
}

/** End-card bullets, possibly derived from what the tour ended up observing. */
export type TourRecap =
  | { label: string; text: string }[]
  | ((app: TourAppState) => { label: string; text: string }[]);

export interface TourDef {
  id: string;
  /** Name in the UI, e.g. "First pipeline". */
  title: string;
  /** One line, for the welcome modal and Settings › Tutorials. */
  blurb: string;
  /** Rough duration in minutes, shown next to Start. */
  minutes: number;
  steps: TourStep[];
  /** The welcome card and its preparations. Absent → the tour starts on step 1. */
  intro?: TourIntro;
  /** End card headline, e.g. "You built …". */
  recapIntro: string | ((app: TourAppState) => string);
  /** End card bullets: what the thing the user just built actually does. */
  recap: TourRecap;
  /**
   * End-card wording, when "done — Finish" is not what the moment is. *First run*
   * ends on something that is *starting*, and its primary just gets out of the way.
   */
  outro?: {
    /** Replaces "<Tour> — done". */
    title?: string;
    /** Replaces "Finish", e.g. "Open the Run". */
    primaryLabel?: string;
    /** A closing sentence under the bullets, pointing at what comes next. */
    closing?: string;
  };
  /**
   * One sentence for the card of a tour that **stopped** while this one is the
   * next leg of a Full tour (#825): what this tour needs, when the previous one
   * failed for want of it. A refusal at Launch is a lesson about the machine, not
   * a reason to lose the chain — and « the next tour does not need a running
   * agent » is the fact that makes Continue worth pressing.
   */
  chainNote?: string;
}

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
