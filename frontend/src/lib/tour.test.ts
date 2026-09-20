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
};

function obs(
  present: string[] = [],
  values: Record<string, string> = {},
  app: Partial<TourAppState> = {},
): TourObservation {
  return {
    app: { ...EMPTY_APP, ...app },
    present: (selector) => present.includes(selector),
    value: (selector) => values[selector] ?? null,
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
    expect(needsConfirm(tour, run)).toBe(true);
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
    expect(needsConfirm(tour, run)).toBe(true);
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
