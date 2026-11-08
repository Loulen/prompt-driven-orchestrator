/**
 * The DOM half of a tour (#823) — the only place a tour touches a browser.
 *
 * The machine in `lib/tour.ts` decides; this hook *observes*: it resolves each
 * step's selectors to rectangles, reads the live value of a field, and pumps the
 * machine on a modest interval plus every scroll and resize. It reads the edit
 * store with `getState()` rather than by subscribing, because the tick already runs
 * often enough and a subscription would re-render the whole tour host on every
 * keystroke in the canvas.
 *
 * Nothing here writes to the app. ADR-0071 §2: the tour observes and blocks; it
 * never fills a field, clicks a button or launches anything.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { dismissTransientOverlays } from "../lib/overlays";
import { canvasNodeIdOf, revealCanvasNode } from "../lib/canvasReveal";
import {
  beginSteps,
  canAdvance,
  confirmStep,
  currentStep,
  needsConfirm,
  observeTour,
  skipStep,
  startTour,
  stepBody,
  stepNote,
  stepSoft,
  type TourRun,
} from "../lib/tour";
import type { TourAppState, TourChecklistItem, TourDef } from "../lib/tourDef";
import { markTourDone } from "../lib/tourMemory";
import { installReadOnlyGuard, type ReadOnlyScope } from "../lib/tourReadOnly";
import {
  HOLE_PADDING,
  MENU_CONTAINERS,
  query,
  rectOf,
  sameAsides,
  sameRect,
  sameRects,
  union,
  visible,
} from "./tour/tourGeometry";
import {
  observation,
  readTourAppState,
  type TourRunDetailInput,
  type TourRunsInput,
} from "./tour/tourObservation";
import { resolvePanels } from "./tour/tourPanels";
import type { TourAsideView, TourController, TourPrepView, TourRect, TourView } from "./tour/tourView";
import { useTourTidyUp } from "./tour/useTourTidyUp";
import { useTourPreparation } from "./useTourPreparation";

/** How often the machine is pumped. Fast enough to feel immediate, cheap enough
 *  that a handful of `querySelector` calls never shows up in a profile. */
const TICK_MS = 100;

const NO_PREP: TourPrepView = { items: [], ready: true, failure: null };
const NO_RECTS: TourRect[] = [];
const NO_ASIDES: TourAsideView[] = [];

export function useTour(
  runs: TourRunsInput,
  runDetail: TourRunDetailInput | null = null,
  launchedRunId: string | null = null,
): TourController {
  const [tour, setTour] = useState<TourDef | null>(null);
  const [run, setRun] = useState<TourRun | null>(null);
  const [hole, setHole] = useState<TourRect | null>(null);
  const [zone, setZone] = useState<TourRect | null>(null);
  const [wideZone, setWideZone] = useState(false);
  const [lit, setLit] = useState<TourRect[]>(NO_RECTS);
  const [asides, setAsides] = useState<TourAsideView[]>(NO_ASIDES);
  const [ready, setReady] = useState(false);
  const [awaitingConfirm, setAwaitingConfirm] = useState(false);
  const [body, setBody] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [checklist, setChecklist] = useState<TourChecklistItem[]>([]);
  const [prep, setPrep] = useState<TourPrepView | null>(null);
  /** The legs of a Full tour still to come, in order. Empty for a lone tour. */
  const [chain, setChain] = useState<TourDef[]>([]);
  // Bumped by `retryPrep` to re-run the preparation effect after a refusal.
  const [prepAttempt, setPrepAttempt] = useState(0);
  // Latest values for the interval, which is installed once per tour and must not
  // churn on every keystroke. Mirrored in effects, never during render
  // (`react-hooks/refs`) — and declared BEFORE the pump effect below so a commit
  // refreshes them first.
  const runRef = useRef<TourRun | null>(null);
  const tourRef = useRef<TourDef | null>(null);
  const runsRef = useRef<TourRunsInput>(runs);
  const runDetailRef = useRef<TourRunDetailInput | null>(runDetail);
  const launchedRunIdRef = useRef<string | null>(launchedRunId);
  /** What the read-only guard protects right now (#911), refreshed by the pump. */
  const guardRef = useRef<ReadOnlyScope | null>(null);
  /** The last aim scrolled to — step and selectors both (see the pump's scroll block). */
  const scrolledFor = useRef<string | null>(null);
  // What the app looked like when this tour started — every observation carries
  // it, so a step can ask "did THIS tour cause that?" (#824).
  const baselineRef = useRef<TourAppState | null>(null);
  useEffect(() => {
    runRef.current = run;
  }, [run]);
  useEffect(() => {
    tourRef.current = tour;
  }, [tour]);
  // The teardown (#911) — see `tour/useTourTidyUp.ts`.
  const { commit, leave, tidyUpFailure, prepFlightRef, prepAbortRef } = useTourTidyUp(tourRef, runRef, setRun);
  useEffect(() => {
    runsRef.current = runs;
  }, [runs]);
  useEffect(() => {
    runDetailRef.current = runDetail;
  }, [runDetail]);
  useEffect(() => {
    launchedRunIdRef.current = launchedRunId;
  }, [launchedRunId]);

  const observe = useCallback(
    () =>
      observation(
        runsRef.current,
        runDetailRef.current,
        launchedRunIdRef.current,
        baselineRef.current ??
          readTourAppState(runsRef.current, runDetailRef.current, launchedRunIdRef.current),
      ),
    [],
  );

  /** Clear what the pump drew: the hole, the zone, the lit panels, the bubbles. */
  const clearAim = useCallback(() => {
    setHole(null);
    setZone(null);
    setWideZone(false);
    setLit(NO_RECTS);
    setAsides(NO_ASIDES);
    guardRef.current = null;
    setChecklist([]);
  }, []);

  const start = useCallback((next: TourDef, rest: TourDef[] = []) => {
    // Starting over a tour that is still up (Settings › Tutorials) is leaving it.
    leave();
    // A tour starts on a clear stage (#825). It points at the app from above
    // every modal, so a transient overlay left open — the `out` artifact the
    // previous leg of a Full tour just told the reader to open — would dim into
    // the background while its full-screen backdrop went on eating every click
    // the first step asks for. Settings closes itself for the same reason; this
    // is that rule for the surfaces that cannot see a tour coming.
    dismissTransientOverlays();
    const baseline = readTourAppState(runsRef.current, runDetailRef.current, launchedRunIdRef.current);
    baselineRef.current = baseline;
    const first = startTour(
      next,
      observation(runsRef.current, runDetailRef.current, launchedRunIdRef.current, baseline),
    );
    scrolledFor.current = null;
    tourRef.current = next;
    runRef.current = first;
    setTour(next);
    setRun(first);
    clearAim();
    setBody("");
    setNote(null);
    // Bumped rather than reset: the attempt counter is what re-arms the
    // preparation effect, and a tour definition is a module constant — so two
    // starts of the SAME tour would otherwise leave the effect's deps untouched
    // and the card waiting forever on preparations that never re-ran.
    setPrepAttempt((n) => n + 1);
    setPrep(next.intro ? null : NO_PREP);
    setChain(rest);
  }, [leave, clearAim]);

  const quit = useCallback(() => {
    leave();
    tourRef.current = null;
    runRef.current = null;
    setTour(null);
    setRun(null);
    clearAim();
    setBody("");
    setNote(null);
    setPrep(null);
    setChain([]);
    baselineRef.current = null;
  }, [leave, clearAim]);

  /**
   * The end card's primary. Marks this tour done, then either chains into the next
   * leg of a Full tour or closes. The checkmark belongs to the tour that was
   * actually completed, so it is written before the next one starts.
   */
  const finish = useCallback(() => {
    if (tour) markTourDone(tour.id);
    const [next, ...rest] = chain;
    if (next) start(next, rest);
    else quit();
  }, [tour, chain, start, quit]);

  /** « Finish here »: same checkmark, no chain (#825). */
  const finishHere = useCallback(() => {
    if (tour) markTourDone(tour.id);
    quit();
  }, [tour, quit]);

  /** « Continue » on a stopped tour's card (#825): the chain goes on, the
   *  checkmark does not get written — nothing was finished. */
  const continueChain = useCallback(() => {
    const [next, ...rest] = chain;
    if (next) start(next, rest);
    else quit();
  }, [chain, start, quit]);

  const next = useCallback(() => {
    if (!tour || !run) return;
    commit(tour, confirmStep(tour, run, observe()));
  }, [tour, run, observe, commit]);

  const skip = useCallback(() => {
    if (!tour || !run) return;
    commit(tour, skipStep(tour, run, observe()));
  }, [tour, run, observe, commit]);

  const begin = useCallback(() => {
    if (!tour || !run) return;
    scrolledFor.current = null;
    commit(tour, beginSteps(tour, run, observe()));
  }, [tour, run, observe, commit]);

  const retryPrep = useCallback(() => {
    setPrep(null);
    setPrepAttempt((n) => n + 1);
  }, []);

  // Preparation (#824) — see `useTourPreparation.ts`.
  const intro = tour?.intro ?? null;
  useTourPreparation(intro, prepAttempt, setPrep, prepFlightRef, prepAbortRef);

  // One pump for everything: the machine, the hole, and the Next button's state.
  // Splitting them would mean three passes over the same `querySelector` results.
  useEffect(() => {
    if (!tour) return;
    const pump = () => {
      const current = runRef.current;
      if (!current || current.phase !== "running") return;
      const obs = observe();
      const observed = observeTour(tour, current, obs, Date.now());
      if (observed !== current) commit(tour, observed);
      const active = runRef.current ?? observed;
      const step = currentStep(tour, active);
      if (!step || active.phase !== "running") {
        clearAim();
        return;
      }

      const selectors = step.target(obs);
      const elements = selectors.map(query).filter(visible);

      const nextHole = union(elements.map(rectOf), HOLE_PADDING);
      setHole((prev) => (sameRect(prev, nextHole) ? prev : nextHole));

      // A portal menu: the dashed zone is the menu the option lives in, resolved
      // from the target rather than declared, so a step never has to know which
      // popup primitive rendered it.
      //
      // With no menu around it, a soft step's zone is the target ITSELF (#825):
      // « you may roam in here » applied to a whole panel. The dim lightens and
      // the ring gives way to the dashed outline, because a ring on a 600-pixel
      // pane would read as "click this", which is the one thing that step is not
      // asking for.
      const soft = stepSoft(step, obs);
      const menu = soft ? elements[0]?.closest(MENU_CONTAINERS) ?? null : null;
      const wide = soft && menu === null && nextHole !== null;
      const nextZone = menu ? union([rectOf(menu)], HOLE_PADDING) : wide ? nextHole : null;
      setZone((prev) => (sameRect(prev, nextZone) ? prev : nextZone));
      setWideZone(wide);

      // The panels its gesture opened (#911): lit read-only, with side bubbles.
      const panels = resolvePanels(step, obs, elements);
      setLit((prev) => (sameRects(prev, panels.lit) ? prev : panels.lit));
      guardRef.current = panels.guard;
      setAsides((prev) => (sameAsides(prev, panels.asides) ? prev : panels.asides));

      setReady(canAdvance(tour, active, obs));
      setAwaitingConfirm(needsConfirm(tour, active, obs));
      setBody(stepBody(step, obs));
      setNote(stepNote(step, obs));
      // Compared item by item: a fresh array every tick would re-render the
      // popover ten times a second for a list that changes twice per tour.
      const nextChecklist = step.checklist?.(obs) ?? [];
      setChecklist((prev) =>
        prev.length === nextChecklist.length &&
        prev.every(
          (item, i) =>
            item.label === nextChecklist[i].label &&
            item.done === nextChecklist[i].done &&
            item.note === nextChecklist[i].note &&
            item.badge === nextChecklist[i].badge,
        )
          ? prev
          : nextChecklist,
      );

      // One scrollIntoView per **aim**, not per step (design Q5, fixed in #824).
      // Following the target continuously would fight the user's own scrolling,
      // so the scroll only ever fires when the step points somewhere new — and a
      // step does point somewhere new while the user works: `pick-repo` rings the
      // explorer dialog while its listing loads, and the `pdo-tutorial` row only
      // once that row exists. Keyed on the step alone, the single scroll was spent
      // on the dialog, and the row stayed wherever an eleven-thousand-pixel `/tmp`
      // had put it — a dimmed explorer with nothing on screen to click.
      //
      // What keeps this from chasing the user is the out-of-view test below, not
      // the key: once a target has been brought into view, re-aiming back onto it
      // scrolls nothing. So the key can be "the last aim" rather than "every aim
      // seen", and a user who climbs out of `/tmp` and back gets the row put in
      // front of them a second time instead of the dimmed explorer again.
      const aim = `${tour.id}:${active.index}:${selectors.join("|")}`;
      if (elements[0] && scrolledFor.current !== aim) {
        scrolledFor.current = aim;
        // A canvas card cannot be scrolled to: it is absolutely positioned inside
        // a transformed viewport, so `scrollIntoView` on it moves nothing and a
        // node outside the visible canvas stays outside it — lit, unreachable, and
        // with the overlay owning the wheel there is no panning out of that (#825,
        // FP iteration 2). The canvas is asked instead, and decides for itself
        // whether the card is already in frame.
        const nodeId = canvasNodeIdOf(elements[0]);
        if (nodeId) {
          revealCanvasNode(nodeId);
        } else {
          const r = elements[0].getBoundingClientRect();
          if (r.top < 0 || r.bottom > window.innerHeight) {
            elements[0].scrollIntoView({ block: "center", behavior: "smooth" });
          }
        }
      }
    };

    pump();
    const timer = window.setInterval(pump, TICK_MS);
    const onViewportChange = () => pump();
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("scroll", onViewportChange, true);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("scroll", onViewportChange, true);
    };
  }, [tour, observe, commit, clearAim]);

  // The read-only guard (#911): one set of capture listeners per tour, reading
  // the scope the pump keeps current. Only a *running* step guards anything —
  // an end card that inherited the last step's scope would keep swallowing the
  // keystrokes of a field the reader has every right to type in again.
  useEffect(() => {
    if (!tour) return;
    return installReadOnlyGuard(() => (runRef.current?.phase === "running" ? guardRef.current : null));
  }, [tour]);

  const view = useMemo<TourView | null>(() => {
    if (!tour || !run) return null;
    return {
      tour,
      run,
      step: currentStep(tour, run),
      stepNumber: run.index + 1,
      total: tour.steps.length,
      hole,
      zone,
      wideZone,
      lit,
      asides,
      ready,
      awaitingConfirm,
      body,
      note,
      checklist,
      prep: run.phase === "intro" ? (prep ?? { items: [], ready: false, failure: null }) : null,
      nextTour: chain[0] ?? null,
      tidyUpFailure,
    };
  }, [tour, run, hole, zone, wideZone, lit, asides, ready, awaitingConfirm, body, note, checklist, prep, chain, tidyUpFailure]);

  return { view, start, quit, next, skip, finish, finishHere, continueChain, begin, retryPrep };
}
