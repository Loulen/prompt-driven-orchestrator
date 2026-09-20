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
  type TourAppState,
  type TourChecklistItem,
  type TourDef,
  type TourObservation,
  type TourRun,
  type TourRunNode,
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
  /**
   * The soft area the hole sits in, drawn dashed: the open menu an option lives
   * in, or — when the step's own target is the zone (#825) — the hole itself.
   */
  zone: TourRect | null;
  /** The zone IS the target: the dim lightens and no ring is drawn (#825). */
  wideZone: boolean;
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
   * The next leg of a **Full tour**, or `null` for a tour started on its own
   * (#824). The end card's primary chains to it instead of closing — that is the
   * whole difference between "the Full tour" and "two tours you start by hand".
   */
  nextTour: TourDef | null;
}

/** What the intro card shows while it prepares what the tour needs (#824). */
export interface TourPrepView {
  items: { id: string; label: string; done: boolean }[];
  /** Every preparation has answered: `Start` is live. */
  ready: boolean;
  /** One refused. The card swaps to its error state and the tour never starts. */
  failure: { title: string; reason: string } | null;
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
 *
 * `[role="dialog"]` joined the list in #824: three of *First run*'s targets sit
 * inside one (the filesystem explorer, the Agent popover, the Skills popover), and
 * those are exactly the surfaces where the user must keep every other control —
 * scroll the tree, climb a folder, press "Select this folder". Nearest ancestor
 * wins, so the New Run modal underneath is never the zone.
 */
const MENU_CONTAINERS =
  '[role="menu"], [role="listbox"], [role="dialog"], [data-slot="dropdown-menu-content"]';

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
/**
 * The Runs the tour is allowed to see — the same list the left panel renders.
 * Only a count and the newest entry are read; the shape is narrowed here so the
 * hook never depends on the full `RunListEntry`.
 */
export interface TourRunsInput {
  length: number;
  0?: { run_id: string; name?: string | null; pipeline_name?: string };
}

/**
 * The selected Run's detail, narrowed to what the reading steps read (#825) —
 * the shape `RunState` already has, minus everything else, so the hook never
 * depends on the full projection. `null` while no Run is open.
 */
export interface TourRunDetailInput {
  run_id: string;
  nodes: Record<
    string,
    {
      node_id: string;
      status: string;
      iter: number;
      iterations: { iter: number; completion_released?: boolean }[];
    }
  >;
  node_defs?: { id: string; name?: string | null; node_type: string }[];
}

/**
 * The Run's nodes, in the order its frozen pipeline declares them, each carrying
 * its status and whether the human has opened its completion guard (ADR-0068).
 *
 * The release flag lives on the *current iteration*, not on the node: a retried
 * node is guarded again, and a tour that read the first iteration's flag would
 * tell the user they had already released a node that is waiting for them.
 */
function runNodes(detail: TourRunDetailInput): TourRunNode[] {
  const defs = detail.node_defs ?? [];
  const order = defs.length > 0 ? defs.map((d) => d.id) : Object.keys(detail.nodes);
  return order.flatMap((id) => {
    const node = detail.nodes[id];
    if (!node) return [];
    const def = defs.find((d) => d.id === id);
    const iteration = node.iterations.find((it) => it.iter === node.iter);
    return [
      {
        id,
        name: def?.name?.trim() || id,
        type: def?.node_type ?? "agent",
        status: node.status,
        released: iteration?.completion_released === true,
      },
    ];
  });
}

export function readTourAppState(
  runs: TourRunsInput,
  detail: TourRunDetailInput | null = null,
): TourAppState {
  const s = useEditStore.getState();
  const tab = s.openTabs.find((t) => t.id === s.activeTabId) ?? null;
  // `GET /runs` answers newest-first, so `[0]` is the Run a tour just caused.
  const newest = runs[0] ?? null;
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
    runCount: runs.length,
    latestRun: newest
      ? {
          id: newest.run_id,
          name: newest.name?.trim() || newest.run_id,
          // Only the detail OF THAT Run: the inspector may well be showing
          // another one, and a tour reading a stranger's nodes would advance on
          // somebody else's progress.
          nodes: detail?.run_id === newest.run_id ? runNodes(detail) : [],
        }
      : null,
    activeRunId: tab?.runId ?? null,
  };
}

function observation(
  runs: TourRunsInput,
  detail: TourRunDetailInput | null,
  baseline: TourAppState,
): TourObservation {
  return {
    app: readTourAppState(runs, detail),
    baseline,
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
    text: (selector) => {
      try {
        const el = document.querySelector(selector);
        // An element that is in the document but has no box is not showing
        // anything to anyone — same rule as `present`.
        return visible(el) ? (el.textContent?.trim() ?? null) || null : null;
      } catch {
        return null;
      }
    },
  };
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

const NO_PREP: TourPrepView = { items: [], ready: true, failure: null };

export function useTour(
  runs: TourRunsInput,
  runDetail: TourRunDetailInput | null = null,
): TourController {
  const [tour, setTour] = useState<TourDef | null>(null);
  const [run, setRun] = useState<TourRun | null>(null);
  const [hole, setHole] = useState<TourRect | null>(null);
  const [zone, setZone] = useState<TourRect | null>(null);
  const [wideZone, setWideZone] = useState(false);
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
  const runsRef = useRef<TourRunsInput>(runs);
  const runDetailRef = useRef<TourRunDetailInput | null>(runDetail);
  /** The last aim scrolled to — step and selectors both (see the pump's scroll block). */
  const scrolledFor = useRef<string | null>(null);
  // What the app looked like when this tour started — every observation carries
  // it, so a step can ask "did THIS tour cause that?" (#824).
  const baselineRef = useRef<TourAppState | null>(null);
  useEffect(() => {
    runRef.current = run;
  }, [run]);
  useEffect(() => {
    runsRef.current = runs;
  }, [runs]);
  useEffect(() => {
    runDetailRef.current = runDetail;
  }, [runDetail]);

  const observe = useCallback(
    () =>
      observation(
        runsRef.current,
        runDetailRef.current,
        baselineRef.current ?? readTourAppState(runsRef.current, runDetailRef.current),
      ),
    [],
  );

  const start = useCallback((next: TourDef, rest: TourDef[] = []) => {
    const baseline = readTourAppState(runsRef.current, runDetailRef.current);
    baselineRef.current = baseline;
    const first = startTour(next, observation(runsRef.current, runDetailRef.current, baseline));
    scrolledFor.current = null;
    setTour(next);
    setRun(first);
    setHole(null);
    setZone(null);
    setWideZone(false);
    setChecklist([]);
    setBody("");
    setNote(null);
    // Bumped rather than reset: the attempt counter is what re-arms the
    // preparation effect, and a tour definition is a module constant — so two
    // starts of the SAME tour would otherwise leave the effect's deps untouched
    // and the card waiting forever on preparations that never re-ran.
    setPrepAttempt((n) => n + 1);
    setPrep(next.intro ? null : NO_PREP);
    setChain(rest);
  }, []);

  const quit = useCallback(() => {
    setTour(null);
    setRun(null);
    setHole(null);
    setZone(null);
    setWideZone(false);
    setChecklist([]);
    setBody("");
    setNote(null);
    setPrep(null);
    setChain([]);
    baselineRef.current = null;
  }, []);

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
    setRun(confirmStep(tour, run, observe()));
  }, [tour, run, observe]);

  const skip = useCallback(() => {
    if (!tour || !run) return;
    setRun(skipStep(tour, run, observe()));
  }, [tour, run, observe]);

  const begin = useCallback(() => {
    if (!tour || !run) return;
    scrolledFor.current = null;
    setRun(beginSteps(tour, run, observe()));
  }, [tour, run, observe]);

  const retryPrep = useCallback(() => {
    setPrep(null);
    setPrepAttempt((n) => n + 1);
  }, []);

  // Preparation (#824): every item of the intro runs in parallel the moment the
  // card opens, and `Start` waits for all of them. Idempotent by contract, so a
  // Retry — or a replayed tour — costs nothing when the work is already done.
  const intro = tour?.intro ?? null;
  useEffect(() => {
    if (!intro) return;
    let cancelled = false;
    const done = new Set<string>();
    const render = (failure: TourPrepView["failure"]) => {
      if (cancelled) return;
      setPrep({
        items: intro.prepare.map((p) => ({
          id: p.id,
          label: done.has(p.id) ? p.ready : p.pending,
          done: done.has(p.id),
        })),
        ready: done.size === intro.prepare.length,
        failure,
      });
    };
    render(null);
    void Promise.all(
      intro.prepare.map((item) =>
        item
          .run()
          .then(() => {
            done.add(item.id);
            render(null);
          })
          .catch((e: unknown) => {
            // The daemon's sentence, quoted. The first refusal wins the card:
            // once it is showing a reason, a second one would only overwrite it.
            if (cancelled) return;
            setPrep((current) =>
              current?.failure
                ? current
                : {
                    items: current?.items ?? [],
                    ready: false,
                    failure: {
                      title: item.failureTitle,
                      reason: e instanceof Error ? e.message : String(e),
                    },
                  },
            );
          }),
      ),
    );
    return () => {
      cancelled = true;
    };
  }, [intro, prepAttempt]);

  // One pump for everything: the machine, the hole, and the Next button's state.
  // Splitting them would mean three passes over the same `querySelector` results.
  useEffect(() => {
    if (!tour) return;
    const pump = () => {
      const current = runRef.current;
      if (!current || current.phase !== "running") return;
      const obs = observe();
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
        setWideZone(false);
        setChecklist([]);
        return;
      }

      const selectors = step.target(obs);
      const elements = selectors
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
      //
      // With no menu around it, a soft step's zone is the target ITSELF (#825):
      // « you may roam in here » applied to a whole panel. The dim lightens and
      // the ring gives way to the dashed outline, because a ring on a 600-pixel
      // pane would read as "click this", which is the one thing that step is not
      // asking for.
      const menu = step.soft ? elements[0]?.closest(MENU_CONTAINERS) ?? null : null;
      const wide = step.soft === true && menu === null && nextHole !== null;
      const nextZone = menu ? union([rectOf(menu)], HOLE_PADDING) : wide ? nextHole : null;
      setZone((prev) => (sameRect(prev, nextZone) ? prev : nextZone));
      setWideZone(wide);

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
  }, [tour, observe]);

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
      ready,
      awaitingConfirm,
      body,
      note,
      checklist,
      prep: run.phase === "intro" ? (prep ?? { items: [], ready: false, failure: null }) : null,
      nextTour: chain[0] ?? null,
    };
  }, [tour, run, hole, zone, wideZone, ready, awaitingConfirm, body, note, checklist, prep, chain]);

  return { view, start, quit, next, skip, finish, finishHere, continueChain, begin, retryPrep };
}
