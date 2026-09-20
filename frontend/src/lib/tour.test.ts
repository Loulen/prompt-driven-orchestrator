/**
 * The tour machine (#823). Every case here is a rule the ticket names:
 * advancement on the observed condition (never on a delay), skippable vs not,
 * a clean stop when the target never appears, and what quitting does.
 */
import { describe, it, expect } from "vitest";
import {
  DEFAULT_TARGET_TIMEOUT_MS,
  canAdvance,
  confirmStep,
  currentStep,
  needsConfirm,
  observeTour,
  skipStep,
  startTour,
  stepSoft,
  type TourAppState,
  type TourDef,
  type TourObservation,
  type TourStep,
} from "./tour";

const EMPTY_APP: TourAppState = {
  pipelineId: null,
  pipeline: null,
  prompts: {},
  selection: { kind: "none", id: null, edgeIndex: null },
  dirty: false,
  libraryPipelineIds: [],
  runCount: 0,
  latestRun: null,
  activeRunId: null,
};

function obs(
  present: string[] = [],
  values: Record<string, string> = {},
  app: Partial<TourAppState> = {},
): TourObservation {
  return {
    app: { ...EMPTY_APP, ...app },
    baseline: EMPTY_APP,
    present: (selector) => present.includes(selector),
    value: (selector) => values[selector] ?? null,
    text: (selector) => values[selector] ?? null,
  };
}

function step(id: string, extra: Partial<TourStep> = {}): TourStep {
  return {
    id,
    title: id,
    body: "",
    target: () => [`#${id}`],
    waitingFor: `the ${id} target`,
    ...extra,
  };
}

function tourOf(...steps: TourStep[]): TourDef {
  return {
    id: "t",
    title: "T",
    blurb: "",
    minutes: 1,
    steps,
    recapIntro: "",
    recap: [],
  };
}

/** The common shape: a step whose condition is a second selector appearing. */
const opensNext = (id: string, marker: string, extra: Partial<TourStep> = {}) =>
  step(id, { done: (o) => o.present(marker), ...extra });

describe("advancing", () => {
  it("moves on when the condition is observed, and not before", () => {
    const tour = tourOf(opensNext("a", "#opened"), step("b"));
    let run = startTour(tour, obs(["#a"]));
    expect(run.index).toBe(0);

    run = observeTour(tour, run, obs(["#a"]), 1_000);
    expect(run.index).toBe(0);

    run = observeTour(tour, run, obs(["#a", "#opened"]), 1_100);
    expect(run.index).toBe(1);
    expect(currentStep(tour, run)?.id).toBe("b");
  });

  it("never advances on time alone — an unmet condition can wait forever", () => {
    const tour = tourOf(opensNext("a", "#opened", { targetTimeoutMs: null }), step("b"));
    let run = startTour(tour, obs(["#a"]));
    for (const now of [1_000, 60_000, 3_600_000]) {
      run = observeTour(tour, run, obs(["#a"]), now);
    }
    expect(run.index).toBe(0);
    expect(run.phase).toBe("running");
  });

  it("finishes after the last step", () => {
    const tour = tourOf(opensNext("a", "#opened"));
    let run = startTour(tour, obs(["#a"]));
    run = observeTour(tour, run, obs(["#a", "#opened"]), 10);
    expect(run.phase).toBe("finished");
  });

  it("advances when the gesture destroys its own target", () => {
    // Clicking Create closes the dialog the step points at. Reading presence
    // first would open a "could not be completed" countdown at the exact moment
    // the user got it right.
    const tour = tourOf(step("create", { done: (o) => o.present("#editor") }), step("b"));
    let run = startTour(tour, obs(["#create"]));
    run = observeTour(tour, run, obs(["#editor"]), 1_000);
    expect(run.index).toBe(1);
  });

  it("stops waiting for a target once a free-input step is satisfied", () => {
    const tour = tourOf(
      step("name", { confirm: true, done: (o) => (o.value("#f") ?? "") !== "" }),
      step("b"),
    );
    let run = startTour(tour, obs(["#name"]));
    const typedThenGone = obs([], { "#f": "tutorial-implement-test" });
    for (const now of [0, 10_000, 20_000]) run = observeTour(tour, run, typedThenGone, now);
    expect(run.phase).toBe("running");
    expect(canAdvance(tour, run, typedThenGone)).toBe(true);
  });

  it("requires every target of a multi-target step", () => {
    const tour = tourOf(
      step("pair", { target: () => ["#one", "#two"], done: (o) => o.present("#linked") }),
      step("b"),
    );
    let run = startTour(tour, obs(["#one"]));
    // Only half the union is on screen: the step is "missing", and the timeout runs.
    run = observeTour(tour, run, obs(["#one"]), 0);
    expect(run.missingSince).toBe(0);
    run = observeTour(tour, run, obs(["#one", "#two"]), 100);
    expect(run.missingSince).toBeNull();
  });
});

describe("free input and acknowledge steps", () => {
  it("waits for Next on a free input, even once the field is filled", () => {
    const tour = tourOf(
      step("name", { confirm: true, done: (o) => (o.value("#f") ?? "") !== "" }),
      step("b"),
    );
    let run = startTour(tour, obs(["#name"]));
    expect(needsConfirm(tour, run, obs(["#name"]))).toBe(true);
    expect(canAdvance(tour, run, obs(["#name"]))).toBe(false);

    const filled = obs(["#name"], { "#f": "tester" });
    run = observeTour(tour, run, filled, 50);
    expect(run.index).toBe(0);
    expect(canAdvance(tour, run, filled)).toBe(true);

    run = confirmStep(tour, run, filled);
    expect(run.index).toBe(1);
  });

  it("refuses Next while the condition is unmet", () => {
    const tour = tourOf(step("name", { confirm: true, done: () => false }), step("b"));
    const run = startTour(tour, obs(["#name"]));
    expect(confirmStep(tour, run, obs(["#name"]))).toBe(run);
  });

  it("a step with no condition is an acknowledge step: Next, always enabled", () => {
    const tour = tourOf(step("read"), step("b"));
    let run = startTour(tour, obs(["#read"]));
    expect(needsConfirm(tour, run, obs(["#read"]))).toBe(true);
    expect(canAdvance(tour, run, obs(["#read"]))).toBe(true);
    run = observeTour(tour, run, obs(["#read"]), 9_999);
    expect(run.index).toBe(0);
    run = confirmStep(tour, run, obs(["#read"]));
    expect(run.index).toBe(1);
  });

  it("does not flash past a step whose condition already held on entry", () => {
    // A new agent node is born isolated, so the "leave it isolated" step arrives
    // already satisfied. Auto-advancing would skip the only thing it teaches.
    const tour = tourOf(step("isolated", { done: () => true }), step("b"));
    let run = startTour(tour, obs(["#isolated"]));
    expect(run.satisfiedOnEntry).toBe(true);
    run = observeTour(tour, run, obs(["#isolated"]), 5_000);
    expect(run.index).toBe(0);
    run = confirmStep(tour, run, obs(["#isolated"]));
    expect(run.index).toBe(1);
  });
});

describe("skipping", () => {
  it("skips an optional step without touching anything", () => {
    const tour = tourOf(step("optional", { skippable: true, done: () => false }), step("b"));
    let run = startTour(tour, obs(["#optional"]));
    run = skipStep(tour, run, obs(["#optional"]));
    expect(run.index).toBe(1);
  });

  it("refuses to skip a step the rest of the tour depends on", () => {
    const tour = tourOf(step("save", { done: () => false }), step("b"));
    const run = startTour(tour, obs(["#save"]));
    expect(skipStep(tour, run, obs(["#save"]))).toBe(run);
  });

  /**
   * #825 — a step may declare that skipping it ends the tour. The case is a
   * *wait*: skipping it means "I have seen enough", and every step after it is
   * about something the thing being waited for has not produced yet. Landing on
   * the recap, which says what was actually observed, beats a failure card.
   */
  it("ends the tour when the skipped step says the rest depends on it", () => {
    const tour = tourOf(
      step("wait", { skippable: true, skipEndsTour: true, done: () => false }),
      step("after", { done: () => false }),
    );
    const run = skipStep(tour, startTour(tour, obs(["#wait"])), obs(["#wait"]));
    expect(run.phase).toBe("finished");
    expect(run.index).toBe(tour.steps.length);
  });
});

describe("a step that decides for itself whether to stop", () => {
  /**
   * #825 — `confirm` as a predicate: the wait step slides on by itself when its
   * subject ended well, and asks for a click when it did not, because the
   * sentence explaining the accident has to be read.
   */
  it("auto-advances or waits for Next depending on what it observed", () => {
    const tour = tourOf(
      step("wait", {
        done: (o) => o.present("#ended"),
        confirm: (o) => o.present("#badly"),
      }),
      step("b"),
    );

    const fine = obs(["#wait", "#ended"]);
    expect(needsConfirm(tour, startTour(tour, obs(["#wait"])), fine)).toBe(false);
    expect(observeTour(tour, startTour(tour, obs(["#wait"])), fine, 10).index).toBe(1);

    const badly = obs(["#wait", "#ended", "#badly"]);
    let run = startTour(tour, obs(["#wait"]));
    run = observeTour(tour, run, badly, 10);
    expect(run.index).toBe(0);
    expect(needsConfirm(tour, run, badly)).toBe(true);
    expect(canAdvance(tour, run, badly)).toBe(true);
    expect(confirmStep(tour, run, badly).index).toBe(1);
  });
});

/**
 * #825 — `soft` as a predicate: a step whose target changes shape under the
 * user. The last step of *First run* rings the `out` row to be clicked, then
 * lights the artifact that opens as a page to read.
 */
describe("a lit area that becomes a zone to roam in", () => {
  it("is resolved against the observation, like the body and the note", () => {
    const opened = step("out", { soft: (o) => o.present("#modal") });
    expect(stepSoft(opened, obs(["#out"]))).toBe(false);
    expect(stepSoft(opened, obs(["#out", "#modal"]))).toBe(true);
  });

  it("still reads a plain flag, and treats its absence as a target to hit", () => {
    expect(stepSoft(step("menu", { soft: true }), obs([]))).toBe(true);
    expect(stepSoft(step("plain"), obs([]))).toBe(false);
  });
});

describe("a target that never appears", () => {
  it("stops the tour cleanly after the timeout, naming what it waited for", () => {
    const tour = tourOf(step("prompt", { failureHint: "The pane was closed." }), step("b"));
    let run = startTour(tour, obs([]));

    run = observeTour(tour, run, obs([]), 1_000);
    expect(run.phase).toBe("running");
    expect(run.missingSince).toBe(1_000);

    run = observeTour(tour, run, obs([]), 1_000 + DEFAULT_TARGET_TIMEOUT_MS);
    expect(run.phase).toBe("failed");
    expect(run.failure).toEqual({
      kind: "missing",
      stepId: "prompt",
      stepNumber: 1,
      total: 2,
      waitingFor: "the prompt target",
      hint: "The pane was closed.",
    });
  });

  it("forgives a target that flickers back before the timeout", () => {
    const tour = tourOf(opensNext("a", "#opened"), step("b"));
    let run = startTour(tour, obs(["#a"]));
    run = observeTour(tour, run, obs([]), 1_000);
    expect(run.missingSince).toBe(1_000);
    run = observeTour(tour, run, obs(["#a"]), 2_000);
    expect(run.missingSince).toBeNull();
    // The clock restarts: the earlier absence is not held against the step.
    run = observeTour(tour, run, obs([]), 3_000);
    expect(run.phase).toBe("running");
    expect(run.missingSince).toBe(3_000);
  });

  it("waits forever when the step says so", () => {
    const tour = tourOf(step("slow", { targetTimeoutMs: null }));
    let run = startTour(tour, obs([]));
    run = observeTour(tour, run, obs([]), 0);
    run = observeTour(tour, run, obs([]), 10 * 60_000);
    expect(run.phase).toBe("running");
  });

  it("is inert once it has stopped", () => {
    const tour = tourOf(step("gone"), step("b"));
    let run = startTour(tour, obs([]));
    run = observeTour(tour, run, obs([]), 0);
    run = observeTour(tour, run, obs([]), DEFAULT_TARGET_TIMEOUT_MS);
    expect(run.phase).toBe("failed");
    expect(observeTour(tour, run, obs(["#gone"]), 99_999)).toBe(run);
    expect(skipStep(tour, run, obs(["#gone"]))).toBe(run);
  });
});
