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
import { useEditStore } from "../stores/editStore";
import {
  canAdvance,
  confirmStep,
  currentStep,
  needsConfirm,
  observeTour,
  skipStep,
  startTour,
  type TourAppState,
  type TourDef,
  type TourObservation,
  type TourRun,
  type TourStep,
} from "../lib/tour";
import { markTourDone } from "../lib/tourMemory";

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
  /** On a `soft` step, the open menu the hole sits in (drawn dashed, clickable). */
  zone: TourRect | null;
  /** The step's condition holds: `Next` is enabled. */
  ready: boolean;
  /** The step waits for `Next` instead of advancing on its own. */
  awaitingConfirm: boolean;
}

/** How often the machine is pumped. Fast enough to feel immediate, cheap enough
 *  that a handful of `querySelector` calls never shows up in a profile. */
const TICK_MS = 100;

/** Breathing room around the hole, so the ring never clips the target's own border. */
const HOLE_PADDING = 6;

/**
 * What a `soft` step's dashed zone is resolved from — the popup the option lives
 * in. The ARIA role is the general answer; `data-slot` is this codebase's own menu
 * (Base UI's `Menu.Popup` via `components/ui/dropdown-menu`), listed so the zone
 * never silently degrades to "block the whole menu but one option" if the
 * primitive's markup shifts.
 */
const MENU_CONTAINERS = '[role="menu"], [role="listbox"], [data-slot="dropdown-menu-content"]';

function rectOf(el: Element): TourRect {
  const r = el.getBoundingClientRect();
  return { top: r.top, left: r.left, width: r.width, height: r.height };
}

function union(rects: TourRect[], padding: number): TourRect | null {
  if (rects.length === 0) return null;
  const top = Math.min(...rects.map((r) => r.top));
  const left = Math.min(...rects.map((r) => r.left));
  const bottom = Math.max(...rects.map((r) => r.top + r.height));
  const right = Math.max(...rects.map((r) => r.left + r.width));
  return {
    top: top - padding,
    left: left - padding,
    width: right - left + padding * 2,
    height: bottom - top + padding * 2,
  };
}

function sameRect(a: TourRect | null, b: TourRect | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    Math.abs(a.top - b.top) < 0.5 &&
    Math.abs(a.left - b.left) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  );
}

/** An element the user can actually aim at: in the document AND laid out. A zero-box
 *  element (a collapsed pane, a menu mid-animation) is "not there yet" for a tour. */
function visible(el: Element | null): el is Element {
  if (!el) return false;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0;
}

/**
 * The slice of app state the step predicates read. Built fresh on every tick from
 * what the UI has already loaded — the tour issues no request of its own.
 */
export function readTourAppState(runCount: number): TourAppState {
  const s = useEditStore.getState();
  const tab = s.openTabs.find((t) => t.id === s.activeTabId) ?? null;
  return {
    pipelineId: tab?.id ?? null,
    pipeline: tab?.pipeline ?? null,
    prompts: tab?.prompts ?? {},
    selection: {
      kind: s.selection.kind,
      id: s.selection.id,
      edgeIndex: s.selection.edgeIndex ?? null,
    },
    dirty: tab?.dirty ?? false,
    libraryPipelineIds: s.pipelines.map((p) => p.id),
    runCount,
  };
}

function observation(runCount: number): TourObservation {
  return {
    app: readTourAppState(runCount),
    present: (selector) => {
      try {
        return visible(document.querySelector(selector));
      } catch {
        return false;
      }
    },
    value: (selector) => {
      try {
        const el = document.querySelector(selector);
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) return el.value;
        return null;
      } catch {
        return null;
      }
    },
  };
}

export interface TourController {
  /** The live view, or null when no tour is running. */
  view: TourView | null;
  start: (tour: TourDef) => void;
  /** Leave without marking anything: an abandoned tour is not a finished one. */
  quit: () => void;
  next: () => void;
  skip: () => void;
  /** The end card's Finish — the only thing that writes the "done" key. */
  finish: () => void;
}

export function useTour(runCount: number): TourController {
  const [tour, setTour] = useState<TourDef | null>(null);
  const [run, setRun] = useState<TourRun | null>(null);
  const [hole, setHole] = useState<TourRect | null>(null);
  const [zone, setZone] = useState<TourRect | null>(null);
  const [ready, setReady] = useState(false);
  const [awaitingConfirm, setAwaitingConfirm] = useState(false);
  // Latest values for the interval, which is installed once per tour and must not
  // churn on every keystroke. Mirrored in effects, never during render
  // (`react-hooks/refs`) — and declared BEFORE the pump effect below so a commit
  // refreshes them first.
  const runRef = useRef<TourRun | null>(null);
  const runCountRef = useRef(runCount);
  const scrolledFor = useRef<string | null>(null);
  useEffect(() => {
    runRef.current = run;
  }, [run]);
  useEffect(() => {
    runCountRef.current = runCount;
  }, [runCount]);

  const start = useCallback((next: TourDef) => {
    const first = startTour(next, observation(runCountRef.current));
    scrolledFor.current = null;
    setTour(next);
    setRun(first);
    setHole(null);
    setZone(null);
  }, []);

  const quit = useCallback(() => {
    setTour(null);
    setRun(null);
    setHole(null);
    setZone(null);
  }, []);

  const finish = useCallback(() => {
    if (tour) markTourDone(tour.id);
    quit();
  }, [tour, quit]);

  const next = useCallback(() => {
    if (!tour || !run) return;
    setRun(confirmStep(tour, run, observation(runCountRef.current)));
  }, [tour, run]);

  const skip = useCallback(() => {
    if (!tour || !run) return;
    setRun(skipStep(tour, run, observation(runCountRef.current)));
  }, [tour, run]);

  // One pump for everything: the machine, the hole, and the Next button's state.
  // Splitting them would mean three passes over the same `querySelector` results.
  useEffect(() => {
    if (!tour) return;
    const pump = () => {
      const current = runRef.current;
      if (!current || current.phase !== "running") return;
      const obs = observation(runCountRef.current);
      const advanced = observeTour(tour, current, obs, Date.now());
      if (advanced !== current) {
        runRef.current = advanced;
        setRun(advanced);
      }
      const active = advanced;
      const step = currentStep(tour, active);
      if (!step || active.phase !== "running") {
        setHole(null);
        setZone(null);
        return;
      }

      const elements = step
        .target(obs.app)
        .map((selector) => {
          try {
            return document.querySelector(selector);
          } catch {
            return null;
          }
        })
        .filter(visible);

      const nextHole = union(elements.map(rectOf), HOLE_PADDING);
      setHole((prev) => (sameRect(prev, nextHole) ? prev : nextHole));

      // A portal menu: the dashed zone is the menu the option lives in, resolved
      // from the target rather than declared, so a step never has to know which
      // popup primitive rendered it.
      const menu = step.soft ? elements[0]?.closest(MENU_CONTAINERS) ?? null : null;
      const nextZone = menu ? union([rectOf(menu)], HOLE_PADDING) : null;
      setZone((prev) => (sameRect(prev, nextZone) ? prev : nextZone));

      setReady(canAdvance(tour, active, obs));
      setAwaitingConfirm(needsConfirm(tour, active));

      // One scrollIntoView per step, on entry only (design Q5): following the
      // target continuously would fight the user's own scrolling.
      const key = `${tour.id}:${active.index}`;
      if (scrolledFor.current !== key && elements[0]) {
        scrolledFor.current = key;
        const r = elements[0].getBoundingClientRect();
        if (r.top < 0 || r.bottom > window.innerHeight) {
          elements[0].scrollIntoView({ block: "center", behavior: "smooth" });
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
      ready,
      awaitingConfirm,
    };
  }, [tour, run, hole, zone, ready, awaitingConfirm]);

  return { view, start, quit, next, skip, finish };
}
